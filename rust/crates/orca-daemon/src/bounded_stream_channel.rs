//! Byte-budgeted per-client stream queue — the memory bound the unbounded
//! `std::sync::mpsc` used before this file did NOT have.
//!
//! Why: `route_output` (registry) enqueues one `StreamItem::Data` per PTY read
//! for the owner and every subscriber, and the drain thread only *coalesces*
//! adjacent items — it never drops. A child flooding its PTY (`yes`,
//! `cat /dev/zero | base64`, ~300 MB/s) while its client stops reading its stream
//! socket (a SIGSTOP'd/wedged Electron, a slow read-only SSH follower) grew the
//! queue without bound → daemon OOM → EVERY live session lost, not just the
//! flooder. This channel bounds it: past the drop cap, the OLDEST `Data` items are
//! dropped down to the keep-tail. `Event` items (an `exit`) are NEVER dropped — a
//! client must always learn its session ended. A hidden pane's stream is only a
//! monitoring feed (reveal restores from the engine snapshot), so shedding its
//! stale backlog is safe — the same policy the Node daemon's
//! `daemon-stream-keep-tail-drop.ts` applies. Kept local (not a dependency on
//! `orca-flow-control`) so the crate's dependency/lock set is untouched.
//!
//! API surface mirrors the `std::sync::mpsc` methods the drain loop and the
//! registry used (`send`/`recv`/`try_recv`, cloneable sender, single receiver),
//! so swapping it in is otherwise transparent.

use crate::stream_coalescing::StreamItem;
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// Past this many queued `Data` bytes, shed oldest `Data` down to the keep-tail.
/// Generous headroom (a client may legitimately back up several sessions and a
/// large paste burst) while still hard-bounding daemon memory per stream socket.
pub const STREAM_QUEUE_DROP_CAP_BYTES: usize = 8 * 1024 * 1024;
/// Retain at least this many trailing `Data` bytes when shedding.
pub const STREAM_QUEUE_KEEP_TAIL_BYTES: usize = 4 * 1024 * 1024;

/// The queued items plus the running total of their `Data` bytes. The two fields
/// are private to this type and change only together (`push_back` / `pop_front` /
/// `drop_oldest_data`), so `data_bytes == Σ len(Data text)` is a local invariant of
/// one small type instead of a convention every channel method must keep.
///
/// The arithmetic is total rather than trusting that invariant: the add is checked
/// (a total that does not fit `usize` is past the drop cap by definition, see
/// `push_back`), and the subtract is checked against the counted total (were the
/// cache ever wrong, it is recomputed from the queue instead of wrapping or
/// panicking — the queue is the source of truth, `data_bytes` only caches its sum).
struct DataQueue {
    queue: VecDeque<StreamItem>,
    /// Sum of `Data` text bytes currently queued — the quantity the cap bounds.
    /// `Event` items are rare and unbounded-safe, so they are not counted.
    data_bytes: usize,
}

impl DataQueue {
    fn new() -> Self {
        DataQueue {
            queue: VecDeque::new(),
            data_bytes: 0,
        }
    }

    /// Enqueue `item`, then shed oldest `Data` down to the keep-tail if the drop
    /// cap is exceeded.
    fn push_back(&mut self, item: StreamItem) {
        match self.data_bytes.checked_add(item_data_len(&item)) {
            Some(total) => {
                self.data_bytes = total;
                self.queue.push_back(item);
                self.enforce_cap();
            }
            // A total past `usize::MAX` is past the drop cap, and this one item is
            // then longer than `usize::MAX - data_bytes`, i.e. far longer than the
            // keep-tail. `enforce_cap` would shed every older `Data` item and then
            // this one too, so do exactly that without forming the sum. (Only a
            // `Data` item has a non-zero length, so this arm never drops an Event.)
            None => self.drop_all_data(),
        }
    }

    fn pop_front(&mut self) -> Option<StreamItem> {
        let item = self.queue.pop_front()?;
        self.release(item_data_len(&item));
        Some(item)
    }

    /// Shed oldest `Data` items down to the keep-tail once the drop cap is
    /// exceeded. `Event` items are stepped over (never dropped, never reordered),
    /// so an `exit` always survives to reach the client.
    fn enforce_cap(&mut self) {
        if self.data_bytes <= STREAM_QUEUE_DROP_CAP_BYTES {
            return;
        }
        while self.data_bytes > STREAM_QUEUE_KEEP_TAIL_BYTES {
            if !self.drop_oldest_data() {
                break; // only Event items remain — nothing droppable
            }
        }
    }

    /// Remove the oldest `Data` item. False when no `Data` item is queued.
    fn drop_oldest_data(&mut self) -> bool {
        let Some(pos) = self.queue.iter().position(is_data) else {
            return false;
        };
        match self.queue.remove(pos) {
            Some(dropped) => {
                self.release(item_data_len(&dropped));
                true
            }
            None => false,
        }
    }

    fn drop_all_data(&mut self) {
        self.queue.retain(|it| !is_data(it));
        self.data_bytes = 0;
    }

    /// Uncount `len` bytes of an item just removed from the queue. The removed
    /// item was counted on entry, so `len <= data_bytes`; were that ever false the
    /// cache is wrong and is rebuilt from what is actually still queued.
    fn release(&mut self, len: usize) {
        self.data_bytes = match self.data_bytes.checked_sub(len) {
            Some(rest) => rest,
            None => self.recount(),
        };
    }

    /// `Σ len(Data text)` over the queue. Saturating: the queued strings all live
    /// in this address space, so their real total is at most `isize::MAX` and the
    /// saturation cannot change the result.
    fn recount(&self) -> usize {
        self.queue
            .iter()
            .map(item_data_len)
            .fold(0, usize::saturating_add)
    }
}

struct Inner {
    data: DataQueue,
    /// Cleared (once) when the last `StreamSender` drops — see `SenderSide`.
    senders_alive: bool,
    /// Cleared when the `StreamReceiver` drops, so senders stop enqueueing.
    receiver_alive: bool,
}

struct Shared {
    inner: Mutex<Inner>,
    signal: Condvar,
}

/// The state every clone of one channel's `StreamSender` shares. Clones share it
/// through an `Arc`, so its `Drop` runs exactly once, when the LAST sender drops:
/// "all senders gone" is carried by `Arc` ownership itself, not by a hand-kept
/// count that every `clone`/`drop` must increment and decrement in step.
struct SenderSide {
    shared: Arc<Shared>,
}

impl Drop for SenderSide {
    fn drop(&mut self) {
        self.shared.inner.lock().unwrap().senders_alive = false;
        // Last sender gone: wake a blocked recv so it can observe disconnect.
        self.shared.signal.notify_one();
    }
}

/// The write half: cloneable (owner + every subscriber share one client queue).
#[derive(Clone)]
pub struct StreamSender {
    side: Arc<SenderSide>,
}

/// The single read half, owned by the stream socket's drain thread.
pub struct StreamReceiver {
    shared: Arc<Shared>,
}

/// `try_recv` outcome, mirroring `std::sync::mpsc::TryRecvError`.
pub enum TryRecvError {
    Empty,
    Disconnected,
}

/// `recv` outcome when every sender has dropped, mirroring `mpsc::RecvError`.
pub struct RecvError;

/// `recv_timeout` outcome, mirroring `mpsc::RecvTimeoutError`.
pub enum RecvTimeoutError {
    Timeout,
    Disconnected,
}

/// `send` outcome when the receiver has dropped, mirroring `mpsc::SendError`:
/// carries the undelivered item but its `Debug` (used by `.unwrap()`) does NOT
/// print the payload, so a giant `Data` chunk isn't dumped on a panic.
pub struct StreamSendError(pub StreamItem);

impl std::fmt::Debug for StreamSendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StreamSendError(..)")
    }
}

pub fn stream_channel() -> (StreamSender, StreamReceiver) {
    let shared = Arc::new(Shared {
        inner: Mutex::new(Inner {
            data: DataQueue::new(),
            senders_alive: true,
            receiver_alive: true,
        }),
        signal: Condvar::new(),
    });
    (
        StreamSender {
            side: Arc::new(SenderSide {
                shared: Arc::clone(&shared),
            }),
        },
        StreamReceiver { shared },
    )
}

fn item_data_len(item: &StreamItem) -> usize {
    match item {
        StreamItem::Data { text, .. } => text.len(),
        StreamItem::Event { .. } => 0,
    }
}

fn is_data(item: &StreamItem) -> bool {
    matches!(item, StreamItem::Data { .. })
}

impl StreamSender {
    /// Enqueue one item. Returns `Err` if the receiver has gone away (parity with
    /// `mpsc::Sender::send`, which errors on a dropped receiver). Under a flooding
    /// producer + stalled consumer this drops oldest `Data` instead of growing
    /// without bound.
    pub fn send(&self, item: StreamItem) -> Result<(), StreamSendError> {
        let shared = &self.side.shared;
        let mut inner = shared.inner.lock().unwrap();
        if !inner.receiver_alive {
            return Err(StreamSendError(item));
        }
        inner.data.push_back(item);
        // Wake a receiver blocked in recv(). One consumer → notify_one suffices.
        shared.signal.notify_one();
        Ok(())
    }

    /// Queued `Data` bytes right now — the bounded quantity, for tests/diagnostics.
    pub fn queued_data_bytes(&self) -> usize {
        self.side.shared.inner.lock().unwrap().data.data_bytes
    }
}

impl StreamReceiver {
    /// Block until an item is available, or every sender has dropped.
    pub fn recv(&self) -> Result<StreamItem, RecvError> {
        let mut inner = self.shared.inner.lock().unwrap();
        loop {
            if let Some(item) = inner.data.pop_front() {
                return Ok(item);
            }
            if !inner.senders_alive {
                return Err(RecvError);
            }
            inner = self.shared.signal.wait(inner).unwrap();
        }
    }

    /// Block until an item is available, every sender drops, or `timeout` elapses.
    /// Mirrors `mpsc::Receiver::recv_timeout`.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<StreamItem, RecvTimeoutError> {
        let deadline = Instant::now() + timeout;
        let mut inner = self.shared.inner.lock().unwrap();
        loop {
            if let Some(item) = inner.data.pop_front() {
                return Ok(item);
            }
            if !inner.senders_alive {
                return Err(RecvTimeoutError::Disconnected);
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(RecvTimeoutError::Timeout);
            }
            let (guard, _timed_out) = self
                .shared
                .signal
                .wait_timeout(inner, deadline - now)
                .unwrap();
            inner = guard;
        }
    }

    /// Non-blocking pop, mirroring `mpsc::Receiver::try_recv`.
    pub fn try_recv(&self) -> Result<StreamItem, TryRecvError> {
        let mut inner = self.shared.inner.lock().unwrap();
        if let Some(item) = inner.data.pop_front() {
            return Ok(item);
        }
        if inner.senders_alive {
            Err(TryRecvError::Empty)
        } else {
            Err(TryRecvError::Disconnected)
        }
    }
}

impl Drop for StreamReceiver {
    fn drop(&mut self) {
        self.shared.inner.lock().unwrap().receiver_alive = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(sid: &str, len: usize) -> StreamItem {
        StreamItem::Data {
            session_id: sid.to_string(),
            text: "x".repeat(len),
        }
    }

    /// THE fix: a receiver that never drains while a producer floods must NOT grow
    /// the queue without bound — queued bytes stay at/under the drop cap.
    #[test]
    fn flooding_a_non_draining_receiver_stays_bounded() {
        let (tx, _rx) = stream_channel();
        // Push ~64 MiB in 64 KiB reads without ever draining — the OOM scenario.
        let chunk = 64 * 1024;
        for _ in 0..1024 {
            tx.send(data("flood", chunk)).unwrap();
            assert!(
                tx.queued_data_bytes() <= STREAM_QUEUE_DROP_CAP_BYTES,
                "queue exceeded the drop cap: {} > {}",
                tx.queued_data_bytes(),
                STREAM_QUEUE_DROP_CAP_BYTES
            );
        }
        // After the flood the retained tail is bounded by the keep-tail band.
        assert!(tx.queued_data_bytes() <= STREAM_QUEUE_DROP_CAP_BYTES);
        assert!(tx.queued_data_bytes() >= STREAM_QUEUE_KEEP_TAIL_BYTES);
    }

    /// An `exit` event is NEVER dropped, even buried under a flood far past the
    /// cap — the client must always learn its session ended.
    #[test]
    fn event_survives_a_flood_and_is_delivered() {
        let (tx, rx) = stream_channel();
        tx.send(StreamItem::Event {
            json: "exit".to_string(),
        })
        .unwrap();
        for _ in 0..512 {
            tx.send(data("flood", 64 * 1024)).unwrap();
        }
        // Drain everything; the event must appear exactly once.
        let mut events = 0;
        loop {
            match rx.try_recv() {
                Ok(StreamItem::Event { .. }) => events += 1,
                Ok(StreamItem::Data { .. }) => {}
                Err(_) => break,
            }
        }
        assert_eq!(events, 1, "the exit event must survive the flood");
    }

    /// Below the cap nothing is dropped: a well-behaved queue is lossless.
    #[test]
    fn under_cap_nothing_is_dropped() {
        let (tx, rx) = stream_channel();
        for i in 0..8 {
            tx.send(data(&format!("s{i}"), 1024)).unwrap();
        }
        drop(tx);
        let mut count = 0;
        while let Ok(_) = rx.recv() {
            count += 1;
        }
        assert_eq!(count, 8, "no item dropped below the cap");
    }

    #[test]
    fn recv_errors_once_all_senders_drop() {
        let (tx, rx) = stream_channel();
        tx.send(data("s", 4)).unwrap();
        drop(tx);
        assert!(rx.recv().is_ok(), "queued item drains first");
        assert!(rx.recv().is_err(), "then disconnect is observed");
    }

    #[test]
    fn send_errors_after_receiver_drops() {
        let (tx, rx) = stream_channel();
        drop(rx);
        assert!(tx.send(data("s", 4)).is_err());
    }

    #[test]
    fn cloned_senders_keep_the_channel_open() {
        let (tx, rx) = stream_channel();
        let tx2 = tx.clone();
        drop(tx);
        tx2.send(data("s", 4)).unwrap();
        assert!(rx.recv().is_ok());
        drop(tx2);
        assert!(rx.recv().is_err(), "last sender gone → disconnected");
    }

    /// The byte total tracks every way an item leaves: shed by the cap, popped
    /// by recv/try_recv/recv_timeout. A full drain returns it to exactly 0.
    #[test]
    fn data_bytes_returns_to_zero_after_mixed_traffic() {
        let (tx, rx) = stream_channel();
        tx.send(StreamItem::Event {
            json: "e1".to_string(),
        })
        .unwrap();
        for _ in 0..200 {
            tx.send(data("s", 64 * 1024 + 7)).unwrap();
        }
        tx.send(StreamItem::Event {
            json: "e2".to_string(),
        })
        .unwrap();
        tx.send(data("s", 3)).unwrap();
        assert!(rx.recv().is_ok());
        assert!(rx.recv_timeout(Duration::from_millis(1)).is_ok());
        while rx.try_recv().is_ok() {}
        assert_eq!(tx.queued_data_bytes(), 0);
    }

    /// Were the cached total ever below what is queued, removal rebuilds it from
    /// the queue (the source of truth) instead of underflowing.
    #[test]
    fn a_short_cached_total_is_recounted_not_underflowed() {
        let mut q = DataQueue::new();
        q.push_back(data("a", 10));
        q.push_back(StreamItem::Event {
            json: "e".to_string(),
        });
        q.push_back(data("b", 5));
        q.data_bytes = 3; // corrupt the cache below the first item's length
        assert!(q.pop_front().is_some());
        assert_eq!(q.data_bytes, 5, "recounted from what is still queued");
        assert!(q.pop_front().is_some()); // the Event: releases 0
        assert!(q.pop_front().is_some());
        assert_eq!(q.data_bytes, 0);
    }

    /// The overflow arm of `push_back` sheds exactly what `enforce_cap` would:
    /// every `Data` item, never an `Event`, and the total is 0.
    #[test]
    fn an_unrepresentable_total_sheds_all_data_but_keeps_events() {
        let mut q = DataQueue::new();
        q.push_back(data("a", 10));
        q.push_back(StreamItem::Event {
            json: "exit".to_string(),
        });
        q.data_bytes = usize::MAX; // so the next Data cannot be added to it
        q.push_back(data("b", 1));
        assert_eq!(q.data_bytes, 0);
        assert_eq!(q.queue.len(), 1);
        assert!(matches!(q.pop_front(), Some(StreamItem::Event { .. })));
    }

    /// Disconnect is carried by `Arc` ownership: a `recv` blocked on an empty
    /// queue wakes with `RecvError` when the last clone drops, not before.
    #[test]
    fn a_blocked_recv_wakes_when_the_last_clone_drops() {
        let (tx, rx) = stream_channel();
        let tx2 = tx.clone();
        let waiter = std::thread::spawn(move || rx.recv().is_err());
        drop(tx);
        std::thread::sleep(Duration::from_millis(20));
        assert!(!waiter.is_finished(), "one sender is still alive");
        drop(tx2);
        assert!(waiter.join().unwrap(), "last sender gone -> disconnected");
    }
}
