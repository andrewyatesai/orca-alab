//! Node-API addon exposing the ATERM-backed `orca_terminal::HeadlessTerminal`
//! to the Electron main/daemon process. Mirrors the surface
//! `src/main/daemon/headless-emulator.ts` needs (write / resize / snapshot /
//! cwd / cursor / mouse-modes / serialize) so it can be swapped in behind the
//! `ORCA_RUST_TERMINAL` flag. This is the real JS -> napi -> aterm path.
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;

const DEFAULT_SCROLLBACK: u32 = 5000;

#[napi(js_name = "HeadlessTerminal")]
pub struct JsHeadlessTerminal {
    inner: orca_terminal::HeadlessTerminal,
}

#[napi]
impl JsHeadlessTerminal {
    /// JS passes (cols, rows); the engine takes (rows, cols) internally.
    #[napi(constructor)]
    pub fn new(cols: u32, rows: u32, scrollback: Option<u32>) -> Self {
        let scrollback = scrollback.unwrap_or(DEFAULT_SCROLLBACK) as usize;
        Self {
            inner: orca_terminal::HeadlessTerminal::with_scrollback(
                rows as usize,
                cols as usize,
                scrollback,
            ),
        }
    }

    #[napi]
    pub fn write(&mut self, data: Buffer) {
        self.inner.process(&data);
    }

    #[napi]
    pub fn resize(&mut self, cols: u32, rows: u32) {
        self.inner.resize(rows as usize, cols as usize);
    }

    /// Visible grid rows (trailing blanks trimmed) — the render snapshot.
    #[napi]
    pub fn snapshot(&self) -> Vec<String> {
        self.inner.snapshot()
    }

    #[napi]
    pub fn scrollback_len(&self) -> u32 {
        self.inner.scrollback_len() as u32
    }

    #[napi]
    pub fn clear_scrollback(&mut self) {
        self.inner.clear_scrollback();
    }

    /// Replayable ANSI for the snapshot (scrollback + visible grid). `&mut` so
    /// the adapter can memoise the result by content-generation + cursor.
    #[napi]
    pub fn serialize_ansi(&mut self) -> String {
        self.inner.serialize_ansi()
    }

    #[napi]
    pub fn cwd(&self) -> Option<String> {
        self.inner.cwd().map(str::to_string)
    }

    /// `[row, col]` cursor position.
    #[napi]
    pub fn cursor(&self) -> Vec<u32> {
        let (r, c) = self.inner.cursor();
        vec![r as u32, c as u32]
    }

    #[napi]
    pub fn mouse_tracking(&self) -> String {
        use orca_terminal::MouseTracking::{Any, Button, Normal, None as MtNone, X10};
        // Capitalised variant names — the daemon factory's RUST_MOUSE_MODE map
        // keys on these (None/X10/Normal/Button/Any).
        match self.inner.mouse_tracking() {
            MtNone => "None",
            X10 => "X10",
            Normal => "Normal",
            Button => "Button",
            Any => "Any",
        }
        .to_string()
    }

    #[napi]
    pub fn sgr_mouse(&self) -> bool {
        self.inner.sgr_mouse()
    }

    #[napi]
    pub fn sgr_pixels(&self) -> bool {
        self.inner.sgr_pixels()
    }

    #[napi]
    pub fn is_alternate_screen(&self) -> bool {
        self.inner.is_alternate_screen()
    }

    #[napi]
    pub fn bracketed_paste(&self) -> bool {
        self.inner.bracketed_paste()
    }

    #[napi]
    pub fn application_cursor(&self) -> bool {
        self.inner.application_cursor()
    }
}

#[napi]
pub fn engine() -> String {
    "aterm".to_string()
}
