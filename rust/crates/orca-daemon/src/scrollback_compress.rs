//! Per-session off-thread scrollback tier-compression worker (audit E1,
//! Wave-3 3A integrator note; THRU-5 pattern mirrored from aterm-gui).
//!
//! With the tiered store attached, scrolled-off lines stage in a lazy buffer
//! and their LZ4/zstd promotion would otherwise run inline in ~1000-line
//! bursts on the pump's PTY-drain critical path — a flood tail-latency spike.
//! The pump instead sets the engine's compress-offload flag and signals this
//! worker, which promotes the backlog in bounded batches, releasing the
//! session-engine lock between batches so the pump and snapshot/checkpoint
//! reads are never starved.
//!
//! Budget semantics (3BC prep): the byte budget is per-`Scrollback`, i.e.
//! per-session; Wave 3 ships per-session budgets only — a daemon-global cap
//! needs registry-level accounting and is deliberately out of scope here.
//! Truncation/pressure surfaces out-of-band via protocol events (E10a), never
//! as sentinel text in the stream.

use crate::registry::SessionEngine;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A capacity-1 signal channel coalesces pump notifications: a token already
/// queued means "a drain is pending", so a `try_send` drop is harmless.
const COMPRESS_SIGNAL_CAP: usize = 1;
/// The pump signals the worker once the deferred backlog reaches this many
/// lines — below the engine's 1000-line inline threshold, so promotion starts
/// before the backlog grows large.
pub const COMPRESS_SIGNAL_AT: usize = 900;
/// Lines promoted per engine-lock hold: each hold costs at most a couple of
/// block compressions, smearing the former one-shot spike across short holds.
const COMPRESS_BUDGET: usize = 256;
/// Stop draining at/below this backlog so the worker never thrashes the lock
/// on the last few lines.
const COMPRESS_LOW_WATER: usize = 256;
/// Signals within this window of each other mean the pump is mid-flood; defer
/// promotion until the stream goes quiet (the cat-flood lesson: compression
/// time-slicing the engine mutex against the PTY drain collapses throughput).
const COMPRESS_QUIET_WINDOW: Duration = Duration::from_millis(50);
/// Mid-flood, promote at most ONE bounded batch this often — forward progress
/// without measurably contending the flood's lock holds.
const COMPRESS_TRICKLE_INTERVAL: Duration = Duration::from_secs(1);

/// Spawn the worker for one session engine. Returns the signal sender the pump
/// holds; dropping it (pump EOF / session teardown) ends the worker. `None` if
/// the thread could not be spawned — the caller must then leave the engine's
/// offload flag INACTIVE so ingest keeps draining inline (pre-E1 behavior).
pub fn spawn_compress_worker(engine: Arc<Mutex<SessionEngine>>) -> Option<SyncSender<()>> {
    let (tx, rx) = sync_channel::<()>(COMPRESS_SIGNAL_CAP);
    std::thread::Builder::new()
        .name("orca-scrollback-compress".into())
        .spawn(move || {
            while rx.recv().is_ok() {
                // Flood gate: wait for the signals to go quiet, trickling one
                // bounded batch per interval so a perpetual flood still makes
                // slow progress (memory stays bounded by the engine's staging
                // backpressure cap meanwhile).
                let mut last_trickle = Instant::now();
                while rx.recv_timeout(COMPRESS_QUIET_WINDOW).is_ok() {
                    if last_trickle.elapsed() >= COMPRESS_TRICKLE_INTERVAL {
                        engine.lock().unwrap().terminal.drain_lazy_bounded(COMPRESS_BUDGET);
                        last_trickle = Instant::now();
                    }
                }
                // Quiet (or teardown — harmless: the engine Arc is still
                // alive): drain to the low-water mark in bounded, lock-yielding
                // batches. Break on NO progress too — while the store is
                // detached for an off-thread reflow the drain is a no-op, and a
                // low-water-only exit would busy-spin the lock all window long.
                let mut prev = usize::MAX;
                loop {
                    let remaining =
                        engine.lock().unwrap().terminal.drain_lazy_bounded(COMPRESS_BUDGET);
                    if remaining <= COMPRESS_LOW_WATER || remaining >= prev {
                        break;
                    }
                    prev = remaining;
                    std::thread::yield_now();
                }
            }
        })
        .ok()
        .map(|_| tx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pending_output::PendingOutput;
    use orca_terminal::HeadlessTerminal;

    /// End-to-end offload wiring: with the flag set and the worker signaled,
    /// the staged backlog is promoted off the feeding thread and the history
    /// text survives intact behind the session-engine lock.
    #[test]
    fn worker_drains_the_offloaded_backlog() {
        let engine = Arc::new(Mutex::new(SessionEngine {
            // Past the fixed hot-ring cap (~1000) so overflow actually stages.
            terminal: HeadlessTerminal::with_scrollback(2, 20, 4000),
            pending: PendingOutput::default(),
        }));
        let tx = spawn_compress_worker(Arc::clone(&engine)).expect("worker spawns");
        {
            let mut e = engine.lock().unwrap();
            e.terminal.set_compress_offload_active(true);
            for i in 0..3000 {
                e.terminal.process_str(&format!("row {i}\r\n"));
            }
            assert!(e.terminal.lazy_backlog_len() > 0, "offload-active ingest stages lines");
        }
        let _ = tx.try_send(());
        // The worker waits out the quiet window, then drains to low water.
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let backlog = engine.lock().unwrap().terminal.lazy_backlog_len();
            if backlog <= COMPRESS_LOW_WATER {
                break;
            }
            assert!(Instant::now() < deadline, "worker never drained: backlog {backlog}");
            std::thread::sleep(Duration::from_millis(20));
        }
        let e = engine.lock().unwrap();
        assert_eq!(e.terminal.scrollback_row_text(0), "row 0");
        assert!(e.terminal.scrollback_len() >= 2900, "history retained through promotion");
        drop(e);
        // Dropping the sender ends the worker (session teardown contract).
        drop(tx);
    }
}
