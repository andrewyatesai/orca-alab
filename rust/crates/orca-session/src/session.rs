use orca_pty::{PtyCommand, PtySession, PtySize};
use orca_terminal::HeadlessTerminal;
use std::io::{self, Read};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// A spawned PTY plus the headless terminal its output feeds. The reader thread
/// owns a cloned PTY reader and parses bytes into the shared terminal; the
/// foreground writes input, resizes, and reads the grid for rendering.
pub struct TerminalSession {
    terminal: Arc<Mutex<HeadlessTerminal>>,
    pty: Arc<Mutex<PtySession>>,
    reader: Option<JoinHandle<()>>,
}

impl TerminalSession {
    pub fn spawn(command: &PtyCommand, rows: u16, cols: u16) -> io::Result<Self> {
        let pty = PtySession::spawn(command, PtySize { rows, cols })?;
        let mut reader = pty.try_clone_reader()?;
        let terminal = Arc::new(Mutex::new(HeadlessTerminal::new(rows as usize, cols as usize)));

        let terminal_for_thread = Arc::clone(&terminal);
        let handle = std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break, // EOF — child exited and PTY drained
                    Ok(n) => {
                        if let Ok(mut terminal) = terminal_for_thread.lock() {
                            terminal.process(&buffer[..n]);
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self { terminal, pty: Arc::new(Mutex::new(pty)), reader: Some(handle) })
    }

    /// Send input bytes to the PTY.
    pub fn write(&self, data: &[u8]) -> io::Result<()> {
        self.pty.lock().expect("pty mutex").write_all(data)
    }

    /// Resize both the grid and the PTY.
    pub fn resize(&self, rows: u16, cols: u16) -> io::Result<()> {
        if let Ok(mut terminal) = self.terminal.lock() {
            terminal.resize(rows as usize, cols as usize);
        }
        self.pty.lock().expect("pty mutex").resize(PtySize { rows, cols })
    }

    /// Read the terminal grid under the lock (for rendering a snapshot/cells).
    pub fn with_terminal<R>(&self, f: impl FnOnce(&HeadlessTerminal) -> R) -> R {
        f(&self.terminal.lock().expect("terminal mutex"))
    }

    // Concrete grid accessors for the FFI boundary (each locks internally).
    pub fn size(&self) -> (usize, usize) {
        self.with_terminal(HeadlessTerminal::size)
    }
    pub fn cursor(&self) -> (usize, usize) {
        self.with_terminal(HeadlessTerminal::cursor)
    }
    pub fn row_text(&self, row: usize) -> String {
        self.with_terminal(|terminal| terminal.row_text(row))
    }
    pub fn cell(&self, row: usize, col: usize) -> Option<orca_terminal::Cell> {
        self.with_terminal(|terminal| terminal.cell(row, col))
    }

    /// Wait for the child to exit and all its output to be processed.
    pub fn wait(&mut self) {
        if let Some(handle) = self.reader.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        // Kill a still-running child so the reader thread unblocks on EOF.
        if let Ok(mut pty) = self.pty.lock() {
            let _ = pty.kill();
        }
        if let Some(handle) = self.reader.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn sh(script: &str) -> PtyCommand {
        PtyCommand {
            program: "/bin/sh".to_string(),
            args: vec!["-c".to_string(), script.to_string()],
            cwd: None,
            env: Vec::new(),
        }
    }

    #[test]
    fn pty_output_streams_into_the_terminal_grid() {
        let mut session = TerminalSession::spawn(&sh("printf hello-session"), 24, 80).expect("spawn");
        session.wait(); // child exits, reader drains and joins
        let row = session.with_terminal(|terminal| terminal.row_text(0));
        assert!(row.contains("hello-session"), "got: {row:?}");
    }

    #[test]
    fn multiline_output_lands_on_separate_rows() {
        let mut session = TerminalSession::spawn(&sh("printf 'line-a\\r\\nline-b'"), 24, 80).expect("spawn");
        session.wait();
        let (r0, r1) = session.with_terminal(|t| (t.row_text(0), t.row_text(1)));
        assert_eq!(r0, "line-a");
        assert_eq!(r1, "line-b");
    }
}
