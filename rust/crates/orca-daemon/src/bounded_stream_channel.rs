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

struct Inner {
    queue: VecDeque<StreamItem>,
    /// Sum of `Data` text bytes currently queued — the quantity the cap bounds.
    /// `Event` items are rare and unbounded-safe, so they are not counted.
    data_bytes: usize,
    /// Live `StreamSender` count; `0` means every sender dropped (disconnected).
    senders: usize,
    /// Cleared when the `StreamReceiver` drops, so senders stop enqueueing.
    receiver_alive: bool,
}

struct Shared {
    inner: Mutex<Inner>,
    signal: Condvar,
}

/// The write half: cloneable (owner + every subscriber share one client queue).
pub struct StreamSender {
    shared: Arc<Shared>,
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
            queue: VecDeque::new(),
            data_bytes: 0,
            senders: 1,
            receiver_alive: true,
        }),
        signal: Condvar::new(),
    });
    (
        StreamSender { shared: Arc::clone(&shared) },
        StreamReceiver { shared },
    )
}

fn item_data_len(item: &StreamItem) -> usize {
    match item {
        StreamItem::Data { text, .. } => text.len(),
        StreamItem::Event { .. } => 0,
    }
}

/// Shed oldest `Data` items down to the keep-tail once the drop cap is exceeded.
/// `Event` items are stepped over (never dropped, never reordered), so an `exit`
/// always survives to reach the client.
fn enforce_cap(inner: &mut Inner) {
    if inner.data_bytes <= STREAM_QUEUE_DROP_CAP_BYTES {
        return;
    }
    while inner.data_bytes > STREAM_QUEUE_KEEP_TAIL_BYTES {
        let Some(pos) = inner
            .queue
            .iter()
            .position(|it| matches!(it, StreamItem::Data { .. }))
        else {
            break; // only Event items remain — nothing droppable
        };
        if let Some(dropped) = inner.queue.remove(pos) {
            inner.data_bytes -= item_data_len(&dropped);
        }
    }
}

impl StreamSender {
    /// Enqueue one item. Returns `Err` if the receiver has gone away (parity with
    /// `mpsc::Sender::send`, which errors on a dropped receiver). Under a flooding
    /// producer + stalled consumer this drops oldest `Data` instead of growing
    /// without bound.
    pub fn send(&self, item: StreamItem) -> Result<(), StreamSendError> {
        let mut inner = self.shared.inner.lock().unwrap();
        if !inner.receiver_alive {
            return Err(StreamSendError(item));
        }
        inner.data_bytes += item_data_len(&item);
        inner.queue.push_back(item);
        enforce_cap(&mut inner);
        // Wake a receiver blocked in recv(). One consumer → notify_one suffices.
        self.shared.signal.notify_one();
        Ok(())
    }

    /// Queued `Data` bytes right now — the bounded quantity, for tests/diagnostics.
    pub fn queued_data_bytes(&self) -> usize {
        self.shared.inner.lock().unwrap().data_bytes
    }
}

impl Clone for StreamSender {
    fn clone(&self) -> Self {
        self.shared.inner.lock().unwrap().senders += 1;
        StreamSender { shared: Arc::clone(&self.shared) }
    }
}

impl Drop for StreamSender {
    fn drop(&mut self) {
        let mut inner = self.shared.inner.lock().unwrap();
        inner.senders -= 1;
        if inner.senders == 0 {
            // Last sender gone: wake a blocked recv so it can observe disconnect.
            self.shared.signal.notify_one();
        }
    }
}

impl StreamReceiver {
    /// Block until an item is available, or every sender has dropped.
    pub fn recv(&self) -> Result<StreamItem, RecvError> {
        let mut inner = self.shared.inner.lock().unwrap();
        loop {
            if let Some(item) = inner.queue.pop_front() {
                inner.data_bytes -= item_data_len(&item);
                return Ok(item);
            }
            if inner.senders == 0 {
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
            if let Some(item) = inner.queue.pop_front() {
                inner.data_bytes -= item_data_len(&item);
                return Ok(item);
            }
            if inner.senders == 0 {
                return Err(RecvTimeoutError::Disconnected);
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(RecvTimeoutError::Timeout);
            }
            let (guard, _timed_out) =
                self.shared.signal.wait_timeout(inner, deadline - now).unwrap();
            inner = guard;
        }
    }

    /// Non-blocking pop, mirroring `mpsc::Receiver::try_recv`.
    pub fn try_recv(&self) -> Result<StreamItem, TryRecvError> {
        let mut inner = self.shared.inner.lock().unwrap();
        if let Some(item) = inner.queue.pop_front() {
            inner.data_bytes -= item_data_len(&item);
            return Ok(item);
        }
        if inner.senders == 0 {
            Err(TryRecvError::Disconnected)
        } else {
            Err(TryRecvError::Empty)
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
        StreamItem::Data { session_id: sid.to_string(), text: "x".repeat(len) }
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
        tx.send(StreamItem::Event { json: "exit".to_string() }).unwrap();
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
}
