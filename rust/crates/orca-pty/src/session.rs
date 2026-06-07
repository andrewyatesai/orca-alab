//! A spawned PTY session wrapping `portable_pty`.

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize as PortablePtySize};
use std::io::{self, Read, Write};

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

/// Spawn parameters, mirroring node-pty's `spawn(file, args, {cwd, env})`.
#[derive(Clone, Debug, Default)]
pub struct PtyCommand {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
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

    /// Write input to the PTY (node-pty's `write`).
    pub fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        self.writer.write_all(data)?;
        self.writer.flush()
    }

    /// Resize the PTY (node-pty's `resize`).
    pub fn resize(&self, size: PtySize) -> io::Result<()> {
        self.master.resize(size.to_portable()).map_err(to_io)
    }

    pub fn process_id(&self) -> Option<u32> {
        self.child.process_id()
    }

    pub fn kill(&mut self) -> io::Result<()> {
        self.child.kill()
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
                env: Vec::new(),
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
    fn resize_succeeds_on_a_live_session() {
        let mut session = PtySession::spawn(
            &PtyCommand {
                program: "/bin/sh".to_string(),
                args: vec!["-c".to_string(), "sleep 0".to_string()],
                cwd: None,
                env: Vec::new(),
            },
            PtySize { rows: 24, cols: 80 },
        )
        .expect("spawn");
        session.resize(PtySize { rows: 40, cols: 120 }).expect("resize");
        let _ = session.wait();
    }
}
