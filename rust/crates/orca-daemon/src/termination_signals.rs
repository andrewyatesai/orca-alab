//! macOS logout / system shutdown SIGTERMs (and SIGHUPs) the detached daemon;
//! with the default disposition it dies without running teardown, and a PTY
//! child that ignores the master-close SIGHUP outlives the logout as an orphan
//! (upstream #7936). Reap the signals on a dedicated `sigwait` thread that runs
//! the same force-kill + socket-unlink path as `shutdown {killSessions:true}`.
//!
//! No `unsafe` handler installation: the signals are blocked via the safe
//! `SigSet::thread_block` / `SigSet::wait` wrappers, keeping this crate under
//! `unsafe_code = "forbid"`.

use crate::registry::Registry;
use nix::sys::signal::{SigSet, Signal};
use std::sync::Arc;
use std::thread;

/// The teardown the signal thread runs — public so tests can exercise it
/// without delivering a real signal to the test process.
pub fn teardown_and_exit_code(registry: &Registry) -> i32 {
    registry.kill_all_sessions();
    registry.unlink_socket();
    0
}

/// Block SIGTERM/SIGHUP and reap them on a dedicated thread.
///
/// Must run on `serve`'s thread before any session/connection thread spawns so
/// every later thread inherits the blocked mask and only the wait thread reaps.
pub fn install(registry: Arc<Registry>) {
    let mut signals = SigSet::empty();
    signals.add(Signal::SIGTERM);
    signals.add(Signal::SIGHUP);
    if signals.thread_block().is_err() {
        // Mask unchanged: the default terminate disposition still applies.
        return;
    }
    thread::spawn(move || loop {
        match signals.wait() {
            Ok(_) => std::process::exit(teardown_and_exit_code(&registry)),
            Err(nix::errno::Errno::EINTR) => continue,
            Err(_) => return,
        }
    });
}
