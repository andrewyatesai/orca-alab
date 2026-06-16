//! Node-API addon exposing the ATERM-backed `orca_terminal::HeadlessTerminal`
//! to Node/Electron. This is the real shipping drop-in path (JS -> napi ->
//! aterm); the bench measures whether the engine's throughput win survives the
//! napi marshalling boundary. Surface mirrors what addon-bench.mjs exercises.
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
}

#[napi]
pub fn engine() -> String {
    "aterm".to_string()
}
