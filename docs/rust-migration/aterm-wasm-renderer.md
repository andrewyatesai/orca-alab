# aterm in-page renderer (replacing xterm.js) — status & plan

Goal: replace **all** of `@xterm/xterm` (the on-screen renderer in `src/renderer`)
with aterm, so aterm is the terminal engine *and* renderer. The daemon-side
`@xterm/headless` swap is already done (aterm via `orca_node.node`); this is the
renderer half — the project's largest remaining lift (XL).

## Architecture (chosen)

Render **in the renderer process** via WASM (lowest latency, SSH-safe — the daemon
keeps the PTY and streams bytes):

```
daemon PTY ──bytes──▶ renderer ──▶ aterm-wasm (aterm-core parse → grid;
                                   aterm-render CPU rasterizer → RGBA Frame)
                                   ──▶ blit to <canvas> (ImageData / WebGL texture)
keyboard/mouse ──▶ renderer ──▶ input bytes ──▶ daemon PTY
```

`aterm-render` is a pure-Rust CPU rasterizer (fontdue + rustybuzz + ttf-parser),
so no GPU/winit/DOM dependency is required for a working renderer. A later
upgrade can use `aterm-gpu` (wgpu → WebGPU/WebGL) behind a capability check.

## Done (this milestone) — the foundation is proven

- **aterm compiles to `wasm32-unknown-unknown`.** Two engine changes (both
  default-on, native byte-identical, TRUST green):
  - `feat(engine): make the render-engine subset wasm32-buildable (gate disk-tier)`
    — the disk cold-tier (libc mmap + zstd-sys C) is now an optional `disk-tier`
    feature, dropped on wasm.
  - `feat(engine): wasm-safe internal clock (web-time)` — internal `Instant`/
    `SystemTime` seams route through `web-time` (std on native, JS clock on wasm)
    so the engine doesn't panic "time not implemented".
- **`aterm/crates/aterm-wasm (in the aterm repo)/`** — a `wasm-bindgen` crate wrapping `aterm-core` +
  `aterm-render`: `new(rows, cols, fontBytes, px)`, `process(bytes)`, `resize`,
  `render()`, `rgba()/width/height`. Builds to wasm; native render test passes.
- **End-to-end proof**: `verify-render.mjs` loads the wasm in Node, feeds colored
  ANSI, rasterizes, and writes a PNG — 528×252px, 118,736 non-bg pixels, 592
  colors. aterm renders a real terminal in wasm.

## Remaining (XL) — phased, behind a flag, main stays on xterm until done

Per the feasibility recon (`@xterm/xterm` is used in ~30 renderer files + ~20 more
coupled to its DOM classes + 8 addons). Sequence:

- **Phase 0 — usable vertical slice**: a canvas-based pane renderer that creates
  the wasm engine, blits frames (dirty-row damage via `render_input_cached`),
  handles fit/resize, keyboard input → PTY, copy/paste (incl. bracketed paste),
  and scroll. Swap the per-pane terminal object in `pane-lifecycle.ts` behind a
  flag (e.g. `ORCA_ATERM_RENDERER`), keeping PaneManager's split/layout DOM.
- **Phase 1**: selection render + clipboard, search UI (wire `aterm-search`) with
  highlight + scroll-to-match, live theme/cursor/font, unicode11 + ZWJ width
  parity (gate on `tools/conformance`).
- **Phase 2**: links (OSC-8 + URL + file-path provider, hover, Cmd/Ctrl-click,
  pixel→cell hit-testing), ligatures, GPU path (`aterm-gpu` → WebGPU/WebGL) with
  CPU fallback.
- **Phase 3**: serialize/restore — emit an xterm-serialize-compatible snapshot (or
  versioned format) for saved-session back-compat; reproduce POST_REPLAY mode
  resets; IME compositionstart positioning. Then remove `@xterm/xterm` + addons.

## Build

```
# wasm renderer foundation
cd aterm/crates/aterm-wasm (in the aterm repo)
cargo build --release --target wasm32-unknown-unknown
wasm-bindgen --target nodejs --out-dir pkg target/wasm32-unknown-unknown/release/aterm_wasm.wasm
node verify-render.mjs   # render proof → /tmp/aterm-wasm-render.png
```
Prereqs: `rustup target add wasm32-unknown-unknown`, `cargo install wasm-bindgen-cli`.
