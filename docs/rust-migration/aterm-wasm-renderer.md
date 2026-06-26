# aterm in-page renderer — status & plan

**Status: aterm is the DEFAULT terminal renderer in the renderer process.** It
draws every on-screen pixel via a GPU path (WebGL2) with an automatic CPU fallback.
`@xterm/xterm` is **not removed** — it is retained, unopened, as an I/O + serialize
+ query-reply shim (see "What xterm.js is still used for"). Full removal is the
remaining Phase 3 lift.

> Honesty note (was an overclaim): earlier drafts of this doc framed the work as
> "replaced xterm.js" and called aterm a "formally verified terminal". Neither is
> accurate as stated. aterm *renders*; xterm is still present as a shim. And the
> engine is **model-checked + proof-assisted + differentially conformance-tested**,
> which is strong but is NOT an end-to-end mechanized refinement proof from spec to
> shipped pixels. See "Verification — what is and isn't proven".

## Architecture (shipped)

Render **in the renderer process** via WASM (lowest latency, SSH-safe — the daemon
keeps the PTY and streams bytes):

```
daemon PTY ──bytes──▶ renderer ──▶ aterm engine (aterm-core parse → grid)
                                   ├─ GPU: aterm-gpu-web (wgpu → WebGL2) ──▶ <canvas>
                                   └─ CPU: aterm-wasm (aterm-render rasterize → RGBA)
                                          ──▶ putImageData(<canvas>)
keyboard/mouse ──▶ renderer ──▶ input bytes ──▶ daemon PTY
```

- **GPU path** (`rust/aterm/crates/aterm-gpu-web`): wgpu with the **WebGL2** backend
  (not WebGPU — Electron gates unsafe-WebGPU; WebGL2 is the safe, ubiquitous
  target). Default via the auto policy in `terminal-webgl-auto-policy.ts`.
- **CPU path** (`rust/aterm/crates/aterm-wasm`): pure-Rust rasterizer (fontdue +
  rustybuzz + ttf-parser), no GPU/DOM dependency. Used as the fallback when the GL
  string is a known software renderer (SwiftShader/llvmpipe/etc., on **all**
  platforms), GPU init fails, or the WebGL2 context is lost at runtime.
- GPU and CPU output are **pixel-equivalent within a small antialiasing/rounding
  tolerance** (≤8 LSB per channel) — the bound the parity tests actually assert
  (`gpu_matches_cpu` checks `delta <= 8`; `aterm-webgl.spec.ts` uses a ±6 tolerance).
  Not bit-identical; only sub-perceptual rounding differs.

## What xterm.js is still used for (the shim — not yet removed)

A single hidden, never-opened `@xterm/xterm` `Terminal` per pane is fed all PTY
output and provides the back-compat surface aterm doesn't yet own:

- **serialize / restore** of saved sessions (xterm-serialize-compatible snapshots).
- **terminal query replies** (CPR / DA1 / DSR / DECRQM) emitted via its `onData`.
- **OSC-7 cwd** tracking and **OSC-52 clipboard** handling.

Honest consequence worth stating plainly: because xterm still owns the replies,
**aterm's own reply layer is currently dead code in the shipped renderer** — its
DA1/DSR/CPR/DECRQM implementation exists and is tested in the engine but is not
wired to the PTY, so the live terminal identity an app sees is xterm's `?1;2c`, not
aterm's. (This is the next thing to fix; see Phase 3.) And both engines parse every
PTY byte per pane — "shim" undersells that xterm is a second full VT parser, not a
passive adapter.

Removing this shim (re-homing serialize + query replies + OSC into aterm) is Phase 3.

## Capabilities wired on the aterm renderer (Phases 0–2, shipped)

- Canvas pane renderer: engine create, dirty-row damage blit, fit/resize, keyboard
  → PTY, copy/paste (incl. bracketed paste), scroll/scrollback.
- Selection (char-drag, **double-click word** via `expand_semantic`, **triple-click
  line** via `expand_lines`), clipboard, search highlight + scroll-to-match.
- Mouse tracking / app-cursor / focus-event modes, OSC-8 + URL link hover &
  Cmd/Ctrl-click, ligatures, unicode/ZWJ width parity.
- Inline images: iTerm2 OSC-1337 + Sixel + Kitty.
- OS fallback fonts injected over IPC (CJK + colour emoji) so non-Latin runs and
  emoji are not tofu.
- Accessibility: an off-screen `role=log` aria-live region mirrors the grid for
  screen readers.

## Performance (measured, honest)

`tests/e2e/aterm-latency.spec.ts` measures single-cell render latency in the live
Electron renderer (ANGLE-Metal / Apple M5 Max at time of writing):

- aterm **GPU** render-half: ~0.2 ms median; per-frame ~0.14 ms @80×24, ~0.24 ms
  @120×40 (stays flat as the grid grows).
- aterm **CPU** render-half: ~7.6 ms median (under one 120 Hz frame); per-frame
  scales with grid area (~7.5 ms @80×24, ~19 ms @120×40).
- xterm + WebGL addon, write→painted **including its rAF debounce**: ~8.5 ms.

Read honestly: the aterm numbers are raw render *work*; the xterm number includes
its one-frame rAF wait, so they measure different things. The takeaway the data
supports: **GPU dominates everywhere and is the default**; the CPU fallback is
competitive with xterm at typical sizes but its rasterization cost exceeds xterm's
frame-bounded paint at large grids — which is precisely why GPU is default and CPU
is only the software-GL fallback.

## Verification — what is and isn't proven

- **Is (strongest layer)**: ~444 `#[kani::proof]` harnesses drive the **real**
  shipped functions (e.g. `Parser::advance` over symbolic input — `parser_never_panics`,
  `params_bounded`), so panic-freedom / bounds on the actual parser are model-checked,
  not just on a paper model.
- **Is (abstract layer)**: hand-written abstract *models* of the VT/grid/mode
  disciplines are model-checked (TLA+ via the `ty` checker), bound to the Rust by
  named proof-anchors and a refinement-coverage ledger.
- **Is**: behaviour is checked against a differential conformance corpus + a fuzzer
  (`tools/conformance`).
- **Isn't**: (1) the TLA+ layer checks the abstract models, **not** the 33k-line
  engine line-for-line — there is no mechanized refinement proof tying the formal
  spec to the exact bytes the shipped wasm renders. (2) The spec gate is fail-closed
  but currently **runs on-demand only**, needs an unpublished local Trust toolchain
  (`~/trust/first-party/{ty,trust-ir}`), and is **not enforced in CI** (this repo's
  workflows were removed). So "model-checked + Kani/SMT proofs on real functions +
  differential conformance" is accurate; "formally verified terminal" and "always-on
  verification ratchet" are not.

## Remaining (Phase 3)

- Re-home serialize/restore, query replies (CPR/DA1/DSR), and OSC-7/OSC-52 into
  aterm; reproduce POST_REPLAY mode resets; IME compositionstart positioning.
- Then remove `@xterm/xterm` + addons (and this shim) entirely.

## Build

aterm sources are vendored into `rust/aterm` and built to wasm by the orc scripts:

```
pnpm bump:aterm              # bump the aterm submodule to latest + rebuild
pnpm run build:aterm-wasm    # build aterm-wasm (CPU) + aterm-gpu-web (GPU), wasm-opt -Oz
pnpm run build:terminal-addon --force
```
