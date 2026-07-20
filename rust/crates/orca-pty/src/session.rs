//! A spawned PTY session wrapping `portable_pty`.

#[cfg(unix)]
use nix::sys::signal::{kill, Signal};
#[cfg(unix)]
use nix::unistd::Pid;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize as PortablePtySize};
use std::io::{self, Read, Write};
#[cfg(unix)]
use std::str::FromStr;

fn to_io(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

/// An owned, `O_NONBLOCK`, `dup`'d read handle over a PTY master, for the daemon
/// pump's gather drain. Owning the dup keeps the master's open file description
/// alive for the pump's whole life, so [`gather_drain`] never reads or polls a
/// closed/recycled fd even if the session entry is torn down concurrently.
/// `O_NONBLOCK` lives on the shared OFD, so it also flips the master's writer to
/// the `EAGAIN`-safe path ([`PtySession::write_all`]). Closes the dup on drop.
#[cfg(unix)]
pub struct MasterReadFd {
    fd: std::os::unix::io::RawFd,
}

#[cfg(unix)]
impl MasterReadFd {
    pub fn as_raw_fd(&self) -> std::os::unix::io::RawFd {
        self.fd
    }
}

#[cfg(unix)]
impl Drop for MasterReadFd {
    fn drop(&mut self) {
        let _ = nix::unistd::close(self.fd);
    }
}

/// Drain the `O_NONBLOCK` master `fd` into `buf` as ONE batch, then report whether
/// the stream ended. The daemon pump calls this instead of a lone blocking `read`:
/// macOS caps each master read at ~1 KiB, so reading one chunk then feeding the
/// engine locksteps the whole pipeline with the writer. Gathering many 1 KiB
/// reads into a 64 KiB batch — with a bounded spin/poll bridge over the writer's
/// microsecond refill gaps — before the (single) engine hand-off ~doubles flood
/// throughput (measured: 182 -> 326 MB/s drain ceiling), matching aterm-pty's
/// `drain_more_nonblocking`.
///
/// `fd` MUST come from [`PtySession::clone_read_fd`] (owned, nonblocking dup).
/// Returns `(filled, eof)`; `eof` is a real end/hard error (the caller stops),
/// never a transient `EAGAIN`.
///   * empty + quiet ⇒ park off-CPU in `poll(POLLIN)` for the first byte or EOF
///     (matches a blocking read at an idle prompt);
///   * `< 1 KiB` at first quiet ⇒ deliver NOW (interactive echo pays no latency);
///   * saturated ⇒ bridge the refill gap with up to 16 immediate re-reads then
///     one 1 ms poll. A batch is bounded by `min(buf.len(), 3 ms of refill-gap
///     bridging)`: sustained gapless output fills `buf` (never touching the
///     budget), while a trickle that keeps bridging delivers after 3 ms.
#[cfg(unix)]
pub fn gather_drain(fd: std::os::unix::io::RawFd, buf: &mut [u8]) -> (usize, bool) {
    use nix::errno::Errno;
    use nix::poll::{PollFd, PollFlags};
    use std::time::{Duration, Instant};
    /// One kernel tty outq — at/above this the writer saturated it.
    const SATURATED: usize = 1024;
    /// Immediate re-read budget per refill gap (aterm's bridge length).
    const SPIN_MAX: u32 = 16;
    /// Hard per-batch latency bound (sub-frame).
    const BUDGET: Duration = Duration::from_millis(3);
    let mut filled = 0usize;
    let mut spins = 0u32;
    let start = Instant::now();
    loop {
        if filled == buf.len() {
            return (filled, false);
        }
        match nix::unistd::read(fd, &mut buf[filled..]) {
            Ok(0) => return (filled, true), // EOF
            Ok(n) => {
                filled += n;
                spins = 0;
            }
            Err(Errno::EINTR) => continue,
            Err(Errno::EAGAIN) => {
                if filled == 0 {
                    // Nothing yet: park off-CPU for the first byte / EOF / HUP.
                    let mut fds = [PollFd::new(fd, PollFlags::POLLIN)];
                    match nix::poll::poll(&mut fds, -1) {
                        Ok(_) => {
                            // A HUP/ERR wake with NO readable data means the child is
                            // gone — stop, so a spurious ready-without-data wake can
                            // never busy-loop re-reading EAGAIN.
                            if let Some(re) = fds[0].revents() {
                                if re
                                    .intersects(PollFlags::POLLHUP | PollFlags::POLLERR | PollFlags::POLLNVAL)
                                    && !re.contains(PollFlags::POLLIN)
                                {
                                    return (0, true);
                                }
                            }
                            continue;
                        }
                        Err(Errno::EINTR) => continue,
                        Err(_) => return (0, true),
                    }
                }
                if filled < SATURATED || start.elapsed() >= BUDGET {
                    return (filled, false);
                }
                if spins < SPIN_MAX {
                    spins += 1;
                    continue;
                }
                // Bridge one refill gap with a bounded poll; quiet ⇒ burst over.
                let mut fds = [PollFd::new(fd, PollFlags::POLLIN)];
                match nix::poll::poll(&mut fds, 1) {
                    Ok(0) | Err(_) => return (filled, false),
                    Ok(_) => spins = 0,
                }
            }
            Err(_) => return (filled, true), // hard error: teardown
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PtySize {
    pub rows: u16,
    pub cols: u16,
}

impl PtySize {
    fn to_portable(self) -> PortablePtySize {
        PortablePtySize { rows: self.rows, cols: self.cols, pixel_width: 0, pixel_height: 0 }
    }
}

/// Spawn parameters, mirroring node-pty's `spawn(file, args, {cwd, env})`. The child
/// inherits the daemon's environment as a base; `env` overrides/adds vars and
/// `env_remove` deletes inherited ones (the daemon's `env` / `envToDelete` payload).
#[derive(Clone, Debug, Default)]
pub struct PtyCommand {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
    pub env_remove: Vec<String>,
}

pub struct PtySession {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
}

impl PtySession {
    /// Open a PTY of `size` and spawn `command` attached to its slave end.
    pub fn spawn(command: &PtyCommand, size: PtySize) -> io::Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(size.to_portable()).map_err(to_io)?;

        let mut builder = CommandBuilder::new(&command.program);
        builder.args(&command.args);
        if let Some(cwd) = &command.cwd {
            builder.cwd(cwd);
        }
        for (key, value) in &command.env {
            builder.env(key, value);
        }
        // Delete inherited vars the caller wants gone (node-pty's envToDelete),
        // applied after sets so a key can't be both added and removed inconsistently.
        for key in &command.env_remove {
            builder.env_remove(key);
        }

        let child = pair.slave.spawn_command(builder).map_err(to_io)?;
        // Drop the slave so the master observes EOF once the child exits.
        drop(pair.slave);
        let writer = pair.master.take_writer().map_err(to_io)?;

        Ok(Self { master: pair.master, child, writer })
    }

    /// A fresh reader over the master end (node-pty's `onData` stream).
    pub fn try_clone_reader(&self) -> io::Result<Box<dyn Read + Send>> {
        self.master.try_clone_reader().map_err(to_io)
    }

    /// An owned, `O_NONBLOCK`, `dup`'d read handle over the master for the daemon
    /// pump's [`gather_drain`]. `dup` (not `try_clone_reader`) so the returned
    /// handle exposes its raw fd for `poll`; owning it keeps the master OFD alive
    /// for the pump's whole life. Setting `O_NONBLOCK` on the shared OFD also
    /// flips the writer to the `EAGAIN`-safe path ([`write_all`](Self::write_all)).
    /// `Err` when portable-pty cannot expose a raw master (e.g. Windows conpty).
    #[cfg(unix)]
    pub fn clone_read_fd(&self) -> io::Result<MasterReadFd> {
        use nix::fcntl::{fcntl, FcntlArg, OFlag};
        let fd = self
            .master
            .as_raw_fd()
            .ok_or_else(|| io::Error::other("master has no raw fd"))?;
        let dup = nix::unistd::dup(fd).map_err(to_io)?;
        // Own the dup from here: every error path must close it (a leaked master
        // OFD keeps a dead child's slave-side resources referenced).
        let raw_flags = match fcntl(dup, FcntlArg::F_GETFL) {
            Ok(v) => v,
            Err(e) => {
                let _ = nix::unistd::close(dup);
                return Err(to_io(e));
            }
        };
        let flags = OFlag::from_bits_truncate(raw_flags);
        if let Err(e) = fcntl(dup, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK)) {
            let _ = nix::unistd::close(dup);
            return Err(to_io(e));
        }
        Ok(MasterReadFd { fd: dup })
    }

    /// Write input to the PTY (node-pty's `write`). `EAGAIN`-safe: once the master
    /// is `O_NONBLOCK` (set by [`clone_read_fd`](Self::clone_read_fd) for the gather
    /// drain), a full tty input queue surfaces `WouldBlock` on the shared fd — park
    /// in `poll(POLLOUT)` and retry rather than drop the write. Plain blocking
    /// masters never see `WouldBlock`, so this is a no-op cost for them.
    ///
    /// CALLER CONTRACT: the `poll(POLLOUT)` park is unbounded (a child that never
    /// drains its stdin blocks the write until it dies), so callers MUST NOT hold a
    /// shared/global lock across this — resolve the session handle, then write.
    pub fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        #[cfg(unix)]
        {
            use nix::errno::Errno;
            use nix::poll::{PollFd, PollFlags};
            let mut off = 0;
            while off < data.len() {
                match self.writer.write(&data[off..]) {
                    Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                    Ok(n) => off += n,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        // WouldBlock implies O_NONBLOCK, which only clone_read_fd sets
                        // and which requires a raw master fd — so `None` here is
                        // impossible; fail loud rather than spin the write forever.
                        let Some(fd) = self.master.as_raw_fd() else {
                            return Err(io::Error::other(
                                "nonblocking write with no pollable master fd",
                            ));
                        };
                        // Park until the tty input queue has room, then retry. EINTR
                        // (process-wide SIGCHLD fires whenever ANY child exits) MUST
                        // retry — aborting here would silently truncate PTY input,
                        // which the old blocking write_all never did (std retries
                        // Interrupted). Matches gather_drain's read-side EINTR loop.
                        let mut fds = [PollFd::new(fd, PollFlags::POLLOUT)];
                        match nix::poll::poll(&mut fds, -1) {
                            Ok(_) | Err(Errno::EINTR) => {}
                            Err(e) => return Err(to_io(e)),
                        }
                    }
                    Err(e) => return Err(e),
                }
            }
            self.writer.flush()
        }
        #[cfg(not(unix))]
        {
            self.writer.write_all(data)?;
            self.writer.flush()
        }
    }

    /// Resize the PTY (node-pty's `resize`).
    pub fn resize(&self, size: PtySize) -> io::Result<()> {
        self.master.resize(size.to_portable()).map_err(to_io)
    }

    pub fn process_id(&self) -> Option<u32> {
        self.child.process_id()
    }

    /// The PID of the terminal's foreground process group (tcgetpgrp) — the command
    /// currently in the foreground (an agent, a build, …), or the shell itself at the
    /// prompt. Feeds the daemon's getForegroundProcess. `None` on platforms/ptys
    /// without the concept.
    #[cfg(unix)]
    pub fn foreground_process_group(&self) -> Option<i32> {
        self.master.process_group_leader()
    }

    /// Windows conpty/winpty has no foreground process group (tcgetpgrp), and
    /// portable-pty only exposes `process_group_leader` on Unix, so report `None`
    /// — the daemon's getForegroundProcess already treats that as "no concept".
    #[cfg(not(unix))]
    pub fn foreground_process_group(&self) -> Option<i32> {
        None
    }

    pub fn kill(&mut self) -> io::Result<()> {
        self.child.kill()
    }

    /// Send a named signal (e.g. "SIGINT"/"SIGTERM"/"SIGWINCH") to the child,
    /// mirroring node-pty's `kill(signal)` — it targets the child pid. A dead
    /// child (no pid) is a silent no-op, matching node-pty's recycled-pid guard.
    #[cfg(unix)]
    pub fn signal(&self, sig: &str) -> io::Result<()> {
        let Some(pid) = self.child.process_id() else {
            return Ok(());
        };
        let signal = Signal::from_str(sig)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, format!("unknown signal: {sig}")))?;
        kill(Pid::from_raw(pid as i32), signal).map_err(to_io)
    }

    /// Windows has no POSIX signal delivery, so this reports the request as
    /// unsupported rather than faking it. The Rust daemon that calls `signal`
    /// runs on Unix only; on Windows the method exists purely to keep the crate
    /// compiling, and the daemon drops the error like a dead-child signal.
    #[cfg(not(unix))]
    pub fn signal(&self, sig: &str) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("POSIX signal delivery ({sig}) is not supported on this platform"),
        ))
    }

    /// Wait for exit and return the child's exit code.
    pub fn wait(&mut self) -> io::Result<u32> {
        Ok(self.child.wait()?.exit_code())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// Read the master end to EOF, tolerating the EIO some platforms raise once
    /// the slave closes.
    fn drain(session: &PtySession) -> String {
        let mut reader = session.try_clone_reader().expect("reader");
        let mut out = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => out.extend_from_slice(&chunk[..n]),
                Err(_) => break,
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    #[test]
    fn spawns_a_command_and_streams_its_output() {
        let mut session = PtySession::spawn(
            &PtyCommand {
                program: "/bin/sh".to_string(),
                args: vec!["-c".to_string(), "printf hello-from-pty".to_string()],
                cwd: None,
                ..PtyCommand::default()
            },
            PtySize { rows: 24, cols: 80 },
        )
        .expect("spawn");
        assert!(session.process_id().is_some());
        let output = drain(&session);
        let _ = session.wait();
        assert!(output.contains("hello-from-pty"), "got: {output:?}");
    }

    #[test]
    fn signal_terminates_a_running_child() {
        let mut session = PtySession::spawn(
            &PtyCommand {
                program: "/bin/sh".to_string(),
                args: vec!["-c".to_string(), "sleep 30".to_string()],
                cwd: None,
                ..PtyCommand::default()
            },
            PtySize { rows: 24, cols: 80 },
        )
        .expect("spawn");
        session.signal("SIGKILL").expect("signal");
        let code = session.wait().expect("wait");
        assert_ne!(code, 0, "a SIGKILL'd child must not report exit 0");
    }

    #[test]
    fn signal_rejects_an_unknown_name() {
        let mut session = PtySession::spawn(
            &PtyCommand {
                program: "/bin/sh".to_string(),
                args: vec!["-c".to_string(), "sleep 0".to_string()],
                cwd: None,
                ..PtyCommand::default()
            },
            PtySize { rows: 24, cols: 80 },
        )
        .expect("spawn");
        assert!(session.signal("NOT_A_SIGNAL").is_err());
        let _ = session.wait();
    }

    #[test]
    fn resize_succeeds_on_a_live_session() {
        let mut session = PtySession::spawn(
            &PtyCommand {
                program: "/bin/sh".to_string(),
                args: vec!["-c".to_string(), "sleep 0".to_string()],
                cwd: None,
                ..PtyCommand::default()
            },
            PtySize { rows: 24, cols: 80 },
        )
        .expect("spawn");
        session.resize(PtySize { rows: 40, cols: 120 }).expect("resize");
        let _ = session.wait();
    }

    /// gather_drain over a flood MUST return every byte, in batches no larger than
    /// the buffer, and report EOF exactly once. NUL bytes (not newlines) so the
    /// tty's OPOST/ONLCR never rewrites the stream and the count is exact.
    #[cfg(unix)]
    #[test]
    fn gather_drain_delivers_a_full_flood_in_bounded_batches_then_one_eof() {
        const N: usize = 200_000; // > 3 * 64 KiB ⇒ several batches
        let session = PtySession::spawn(
            &PtyCommand {
                program: "/bin/sh".to_string(),
                args: vec!["-c".to_string(), format!("head -c {N} /dev/zero")],
                cwd: None,
                ..PtyCommand::default()
            },
            PtySize { rows: 24, cols: 80 },
        )
        .expect("spawn");
        let read_fd = session.clone_read_fd().expect("clone_read_fd");
        let fd = read_fd.as_raw_fd();
        let mut buf = [0u8; 65536];
        let mut total = 0usize;
        let mut eofs = 0usize;
        loop {
            let (filled, eof) = gather_drain(fd, &mut buf);
            assert!(filled <= buf.len(), "batch overran the buffer");
            total += filled;
            if eof {
                eofs += 1;
                break;
            }
        }
        assert_eq!(total, N, "gather must deliver every flooded byte");
        assert_eq!(eofs, 1, "EOF reported exactly once");
    }
}
