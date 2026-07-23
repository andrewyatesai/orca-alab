//! A spawned PTY session wrapping `portable_pty`.

#[cfg(unix)]
use nix::sys::signal::{kill, Signal};
#[cfg(unix)]
use nix::unistd::Pid;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize as PortablePtySize};
use std::io::{self, Read, Write};
#[cfg(unix)]
use std::str::FromStr;
use std::sync::{Arc, Mutex};

/// A cloneable, independently-lockable handle to a PTY's input writer. The daemon
/// writes stdin under THIS lock — never the global registry lock — so a child that
/// stopped draining its tty (a SIGSTOP'd foreground process, `^S`/IXON) can block a
/// `write_all` on the full kernel PTY buffer without wedging every other session.
pub type PtyWriter = Arc<Mutex<Box<dyn Write + Send>>>;

fn to_io(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
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
    writer: PtyWriter,
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
        let writer = Arc::new(Mutex::new(pair.master.take_writer().map_err(to_io)?));

        Ok(Self { master: pair.master, child, writer })
    }

    /// A clone of the input-writer handle so callers can write stdin under the
    /// per-session writer lock alone, off any shared lock they may hold. See
    /// `PtyWriter`: a blocking write to a stalled child must not wedge the daemon.
    pub fn writer_handle(&self) -> PtyWriter {
        Arc::clone(&self.writer)
    }

    /// A fresh reader over the master end (node-pty's `onData` stream).
    pub fn try_clone_reader(&self) -> io::Result<Box<dyn Read + Send>> {
        self.master.try_clone_reader().map_err(to_io)
    }

    /// The master's raw fd (unix), for diagnostics/benches that need to `poll` or
    /// `fcntl` the master directly (e.g. the pump-drain investigation bench).
    /// `None` when portable-pty cannot expose it (Windows conpty).
    #[cfg(unix)]
    pub fn master_raw_fd(&self) -> Option<std::os::unix::io::RawFd> {
        self.master.as_raw_fd()
    }

    /// Write input to the PTY (node-pty's `write`). Takes `&self` (the writer is
    /// behind its own lock), so a caller can write without an exclusive borrow.
    pub fn write_all(&self, data: &[u8]) -> io::Result<()> {
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(data)?;
        writer.flush()
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
}
