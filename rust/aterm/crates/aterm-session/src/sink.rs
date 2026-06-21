// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! The single serialization point for all bytes entering one session's PTY master
//! (design §6.3).

use std::io;
use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::Mutex;

/// The ONE place bytes enter a session's PTY master fd. Every writer — the GUI
/// keyboard, every control verb, the future `keys` forwarder, and the reader
/// thread's query replies — funnels through [`SinkWriter::write_frame`], so two
/// writers can never interleave bytes INSIDE one frame (whole-frame atomicity).
/// Without this, two edges writing prompts larger than `PIPE_BUF` (512 on Darwin)
/// would shred each other — the multi-writer-corruption hazard the design calls out.
///
/// Ordering guarantee: **total order per sink, arbitrary fairness across writers,
/// whole-frame atomicity**.
///
/// ## Phase 0 scope (honest)
///
/// The master is left in BLOCKING mode and `write_frame` writes the whole frame
/// under the lock. The non-blocking ring + per-edge token bucket + `poll(2)`
/// deselect that give end-to-end backpressure (design §6.2) layer on top in a later
/// milestone via [`aterm_pty::set_nonblocking`] + a drained ring — the
/// [`aterm_pty::write_some`] seam this is built on already returns the true accepted
/// count, so adding that layer does not change this interface.
///
/// ## fd ownership (the close-vs-use race fix)
///
/// A sink built with [`SinkWriter::new_owned`] OWNS the master fd: it is closed
/// exactly when the LAST `Arc<SinkWriter>` clone drops (via the held [`OwnedFd`]) —
/// never by an out-of-band `close()`. Every party that uses the master holds an
/// `Arc<SinkWriter>` clone (the session reader thread, each window's mirror, each
/// in-flight control verb), so the fd number cannot be freed — and therefore cannot
/// be recycled by a subsequent `forkpty` — while any reader is parked in
/// `read(master)` or any writer is inside `write_frame`. The GUI/reader still use
/// the RAW fd (via [`SinkWriter::master`]) for read/resize, valid for exactly as
/// long as they hold their clone. (Previously `Session::drop` `close()`d a bare
/// `i32` on a detached thread, racing the still-parked reader and the live sink
/// mirrors — a recycled fd could then route a read or a keystroke to the WRONG
/// session.) [`SinkWriter::new`] keeps the old BORROWED semantics (no close) for
/// test stubs and sentinel (`-1`) fds.
pub struct SinkWriter {
    /// The raw master fd, used directly for write/read/resize. Equals the owned fd's
    /// number when `_owned` is `Some`; a borrowed/sentinel number otherwise.
    master: i32,
    /// Ownership token: `Some` iff this sink OWNS the fd (built via `new_owned`), in
    /// which case dropping the last `Arc<SinkWriter>` closes it. `None` for borrowed
    /// fds / `-1` stubs (no close — unchanged legacy behavior). Held only for its
    /// `Drop`; never read.
    _owned: Option<OwnedFd>,
    /// Serializes whole frames. Held for the duration of one `write_frame` so no
    /// other writer's bytes interleave. A poisoned lock is recovered (we never panic
    /// a writer thread for the fd's sake); the invariant it guards is "one frame at a
    /// time", which a recovered guard still upholds.
    lock: Mutex<()>,
}

impl SinkWriter {
    /// Wrap a BORROWED PTY master fd: this sink does NOT close it (the caller — or a
    /// `-1` sentinel — retains ownership). The legacy constructor, used by tests and
    /// by sink stubs that don't drive a real PTY.
    #[must_use]
    pub fn new(master: i32) -> Self {
        Self { master, _owned: None, lock: Mutex::new(()) }
    }

    /// Take OWNERSHIP of a PTY master fd (passed as an [`OwnedFd`], so this crate
    /// stays `forbid(unsafe_code)` — the caller does the one `from_raw_fd`): the fd
    /// is closed exactly when the last `Arc<SinkWriter>` clone drops (see the type
    /// docs). Use this for a fd the caller owns and must NOT `close()` elsewhere
    /// (e.g. a `forkpty` master).
    ///
    /// SPEC (initiative A7, WS-G): this constructor establishes the OwnedFd-RAII
    /// ownership discipline modeled by `fd_lifecycle_model()` (machine
    /// `fd_lifecycle` / `FdLifecycle`). Its two RAII actions have NO aterm method to
    /// bind — they ARE the std `Arc::clone` and `OwnedFd::drop` the discipline rides
    /// on — so they are waived here (the `master()`/`write_frame` fd-USE action is
    /// the real `#[refines]` anchor, on those methods). This covers the model's
    /// Clone/DropClone actions for the closure gate's coverage obligation (Ob.3).
    #[cfg_attr(
        any(test, feature = "spec-anchors"),
        aterm_spec::spec_unmodeled(
            machine = "fd_lifecycle",
            action = "Clone",
            reason = "RAII, no aterm method to anchor: the Clone action is std `Arc::clone(&sink)` \
                      taken by each holder (the reader thread, each window mirror, each in-flight \
                      control verb). It only increments the live strong count — there is no \
                      SinkWriter method to bind a #[refines] to. The fd-USE this clone authorizes \
                      IS modeled+anchored (UseFd -> master()/write_frame). Waived so the model's \
                      Clone action is covered (Ob.3) without inventing a no-op wrapper."
        )
    )]
    #[cfg_attr(
        any(test, feature = "spec-anchors"),
        aterm_spec::spec_unmodeled(
            machine = "fd_lifecycle",
            action = "DropClone",
            reason = "RAII, no aterm method to anchor: the DropClone action is the std `Drop` of an \
                      `Arc<SinkWriter>` clone; THE FIX is that the held `OwnedFd` (this field) closes \
                      the fd EXACTLY when the LAST clone drops (sink.rs:32-39, exercised by the \
                      `owned_fd_stays_open_until_last_clone_drops` regression). The close is \
                      `OwnedFd::drop`, not an aterm method, so there is nothing to #[refines]. Waived \
                      so the model's DropClone action is covered (Ob.3)."
        )
    )]
    #[must_use]
    pub fn new_owned(master: OwnedFd) -> Self {
        Self { master: master.as_raw_fd(), _owned: Some(master), lock: Mutex::new(()) }
    }

    /// PROJECTION (TRUST_VACUITY_GATE §2.2 / L2): the `&SinkWriter` → derived
    /// `fd_lifecycle_model` abstract-state witness for the `UseFd` `#[refines]`
    /// anchors below (`master()` / `write_frame`). It maps the live sink onto the
    /// model's `<<fdOpen, hasOwner>>` observables:
    ///
    ///   * `fd_open` — whether this sink still names a usable master fd (`master != -1`):
    ///     the model's `fdOpen` from the holder's vantage. A `UseFd` is sound exactly
    ///     when this is `true`, which the OwnedFd-last-drop discipline guarantees while
    ///     any clone is alive (so `usedAfterClose` never latches — `NoUseAfterClose`).
    ///   * `owns_fd` — whether this sink OWNS the fd (built via `new_owned`): the
    ///     `_owned` token whose `Drop` on the last clone is the model's `DropClone`
    ///     close-on-last-drop.
    ///
    /// The live Arc strong count (the model's `clones`) is NOT observable from
    /// `&self` (it lives in the `Arc` the caller holds), so it is intentionally out of
    /// the structural projection — exactly the partial-projection shape the fork_exec
    /// witness uses for its child program-counter (L2 requires a real projection
    /// NAME, not its execution; the BEHAVIORAL guarantee is the Tier-0 `ty` proof +
    /// the `owned_fd_stays_open_until_last_clone_drops` regression).
    #[must_use]
    pub fn project_fd_state(&self) -> (bool, bool) {
        (self.master != -1, self._owned.is_some())
    }

    /// The wrapped master fd (for callers that read/resize it directly). For an
    /// owned sink it is valid for as long as the caller holds its `Arc<SinkWriter>`
    /// clone — the fd cannot close out from under it while a clone is alive.
    ///
    /// SPEC (A7): handing out the RAW master fd for read/resize is the model's
    /// `UseFd` action — a holder using the raw fd. The OwnedFd-last-drop discipline
    /// (modeled by `fd_lifecycle_model`) is what makes this sound: while any clone is
    /// alive the fd is open, so `usedAfterClose` can never latch (NoUseAfterClose).
    #[cfg_attr(
        any(test, feature = "spec-anchors"),
        aterm_spec::refines(
            machine = "fd_lifecycle",
            action = "UseFd",
            project = "aterm_session::sink::SinkWriter::project_fd_state"
        )
    )]
    #[must_use]
    pub fn master(&self) -> i32 {
        self.master
    }

    /// Write a WHOLE frame atomically with respect to other writers, returning the
    /// number of bytes accepted (`== bytes.len()` on success). Holds the
    /// serialization lock for the duration, so no other writer's bytes can appear
    /// inside this frame. Propagates the first hard error rather than silently
    /// dropping the tail (the bug the legacy `write_all` had before `write_some`).
    ///
    /// On a blocking master (Phase 0) this only returns early on a hard error or a
    /// `0` write (peer closed). A `WouldBlock` (a caller set `O_NONBLOCK` for the
    /// future backpressure layer) is surfaced to the caller rather than spun on.
    ///
    /// SPEC (A7): writing through the raw master fd is the model's `UseFd` action.
    /// The OwnedFd-last-drop discipline guarantees the fd is open for the whole
    /// duration any clone (including this writer's) is alive, so the use can never
    /// land on a closed/recycled fd — the `NoUseAfterClose` invariant of
    /// `fd_lifecycle_model`.
    #[cfg_attr(
        any(test, feature = "spec-anchors"),
        aterm_spec::refines(
            machine = "fd_lifecycle",
            action = "UseFd",
            project = "aterm_session::sink::SinkWriter::project_fd_state"
        )
    )]
    pub fn write_frame(&self, bytes: &[u8]) -> io::Result<usize> {
        let _guard = self.lock.lock().unwrap_or_else(|poison| poison.into_inner());
        let mut off = 0;
        while off < bytes.len() {
            match aterm_pty::write_some(self.master, &bytes[off..]) {
                Ok(0) => break, // peer closed mid-frame
                Ok(n) => off += n,
                Err(e) => return Err(e),
            }
        }
        Ok(off)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::os::fd::AsRawFd;
    use std::sync::Arc;
    use std::thread;

    // Whole-frame atomicity: N threads each write a distinct frame LARGER than the
    // socket send buffer (so a single `write` short-writes and the loop iterates,
    // giving the kernel real opportunities to interleave two writers). Through one
    // SinkWriter the bytes must arrive as exactly N CONTIGUOUS single-byte runs;
    // without the serialization lock the runs would fragment. Driven on a stream
    // socketpair (no shell, no unsafe — fds are borrowed from owned `UnixStream`s),
    // with a concurrent reader so the oversized writes never deadlock on a full buffer.
    #[test]
    fn write_frame_is_whole_frame_atomic_across_threads() {
        let (mut reader, writer) =
            std::os::unix::net::UnixStream::pair().expect("socketpair");
        // Borrowed fd (the owned `writer` is dropped below) — `new` doesn't close it.
        let sink = Arc::new(SinkWriter::new(writer.as_raw_fd()));

        const N: u8 = 4;
        const LEN: usize = 128 * 1024; // > any default socket buffer -> forces short writes

        // Drain concurrently so the oversized frames don't block on a full buffer.
        let reader_handle = thread::spawn(move || {
            let mut buf = vec![0u8; (N as usize) * LEN];
            reader.read_exact(&mut buf).expect("read_exact");
            buf
        });

        let mut handles = Vec::new();
        for i in 0..N {
            let s = Arc::clone(&sink);
            handles.push(thread::spawn(move || {
                let frame = vec![b'A' + i; LEN];
                assert_eq!(s.write_frame(&frame).expect("write_frame"), LEN, "whole frame accepted");
            }));
        }
        for h in handles {
            h.join().expect("writer thread");
        }
        let buf = reader_handle.join().expect("reader thread");
        drop(writer); // keep the borrowed fd alive until here

        let runs = runs_of(&buf);
        assert_eq!(
            runs.len(),
            N as usize,
            "expected {N} contiguous frames; interleaving fragmented them into {} runs",
            runs.len()
        );
        for (byte, len) in &runs {
            assert_eq!(*len, LEN, "frame for byte {byte} was split — writers interleaved");
        }
        let mut distinct: Vec<u8> = runs.iter().map(|(b, _)| *b).collect();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), N as usize, "every frame's byte must appear exactly once");
    }

    // Run-length summary [(byte, count), ...] of consecutive equal bytes.
    fn runs_of(buf: &[u8]) -> Vec<(u8, usize)> {
        let mut runs: Vec<(u8, usize)> = Vec::new();
        for &b in buf {
            match runs.last_mut() {
                Some((rb, n)) if *rb == b => *n += 1,
                _ => runs.push((b, 1)),
            }
        }
        runs
    }

    #[test]
    fn write_frame_reports_full_count() {
        let (mut reader, writer) =
            std::os::unix::net::UnixStream::pair().expect("socketpair");
        let sink = SinkWriter::new(writer.as_raw_fd());
        assert_eq!(sink.write_frame(b"hello-sink").expect("write_frame"), 10);
        let mut buf = [0u8; 10];
        reader.read_exact(&mut buf).expect("read_exact");
        assert_eq!(&buf, b"hello-sink");
        drop(writer);
    }

    // REGRESSION (integration audit): a `new_owned` SinkWriter OWNS the fd and closes
    // it only when the LAST Arc clone drops — never out-of-band. So while ANY clone is
    // alive (a parked reader, a window mirror, an in-flight control verb), the fd
    // number stays valid and cannot be recycled by a later forkpty. This is what
    // prevents a close-vs-read/write race from routing a read or keystroke to the
    // WRONG session.
    #[test]
    fn owned_fd_stays_open_until_last_clone_drops() {
        let (mut reader, writer) = std::os::unix::net::UnixStream::pair().expect("socketpair");
        // Safe owning conversion (no unsafe — this crate is forbid(unsafe_code)):
        // the SinkWriter takes the writer end's OwnedFd and is its sole owner.
        let owned: OwnedFd = writer.into();
        let sink = Arc::new(SinkWriter::new_owned(owned));
        let clone = Arc::clone(&sink);

        // Drop the original Arc: a clone remains, so the fd MUST still be open+writable.
        drop(sink);
        assert_eq!(clone.write_frame(b"alive").expect("write while a clone holds the fd"), 5);

        // Drop the LAST clone: the OwnedFd closes the fd exactly once. The peer then
        // reads the 5 bytes and EOF (read_to_end returns) — which only happens because
        // the write end was closed on the last clone drop. (A leak would hang here.)
        drop(clone);
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).expect("read_to_end");
        assert_eq!(&buf, b"alive", "peer got the bytes then EOF — fd closed on last clone drop");
    }
}
