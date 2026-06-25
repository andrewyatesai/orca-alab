<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Andrew Yates -->

# Proof-Carrying Performance — the aterm verification boundary

This document defines, precisely and without overclaiming, **what is formally
verified about Orca's aterm terminal engine, what is deliberately out of scope
for those proofs, and how the out-of-scope band is covered instead.**

It is the anchor that the re-checkable proof bundles point back to: the `ay`
SMT/CHC certificates under
[`aterm/crates/aterm-spec-models/proofs/ay/`](aterm/crates/aterm-spec-models/proofs/ay/)
reference this file (e.g. `proofs/ay/README.md`, `proofs/ay/a9_strip_appearance/README.md`,
and `proofs/ay/verify.sh` all cite `PROOF_CARRYING_PERFORMANCE.md`).

The single organizing idea: **the proof boundary is the wasm FFI surface.** The
Rust engine is verified over an *abstract* domain — bytes in, grid cells / cursor
/ response bytes out, plus an already-scaled integer cell size. Everything that is
device- or DOM-dependent (device-pixel-ratio handling, glyph rasterization
*quality* at a given scale, and renderer↔DOM layout *measurement*) lives in
TypeScript on the renderer side of that FFI and is owned by TS unit + Playwright
e2e gates, not by an engine theorem.

---

## Scope

The aterm engine is split across two trust domains separated by the wasm FFI:

1. **The Rust engine** (`aterm-core`, `aterm-grid`, `aterm-render`, `aterm-gpu`,
   `aterm-types`, `aterm-codec`, …). It consumes a **byte stream** and exposes a
   **read API** over an abstract grid. Its semantics are exhaustively tested by a
   conformance oracle and certified, in specific spots, by machine-checked SMT/CHC
   proofs and TLA+ models.

2. **The TypeScript renderer seam** (`src/renderer/src/lib/pane-manager/aterm/`
   and `src/renderer/src/components/terminal-pane/`). It measures the DOM,
   computes a grid size, scales fonts by `devicePixelRatio`, and feeds the engine
   an **already-scaled integer pixel size** plus an abstract `(cols, rows)` grid
   through the FFI.

The verification boundary runs through the FFI calls `set_px(px)`, `cell_width`,
and `cell_height`. **The engine receives an integer pixel size that has already
been scaled by the renderer; it never sees a CSS measurement, a DOM container, or
`devicePixelRatio`.** Everything the engine proves is therefore stated over the
abstract grid, independent of the physical device.

---

## What the Rust proofs verify

### 1. Terminal semantics — the conformance oracle

The load-bearing guarantee is **semantic correctness over the domain
`{byte stream in} → {grid cells / cursor / response bytes out}`**. The oracle is
the engine's own read API, wrapped by the conformance harness
([`aterm/crates/aterm-conformance/src/lib.rs`](aterm/crates/aterm-conformance/src/lib.rs)):
a test is `feed(bytes) → read the screen → assert`. The `Screen` wrapper exposes
exactly the observable surface:

- `row(r)` / `screen()` — visible cell text;
- `cursor()` — cursor `(row, col)`;
- `style_fingerprint()` — the resolved `(fg, bg, flags)` SGR state;
- `cell_flags_bits(r, c)` / `hyperlink_at(r, c)` / `images_row(r)` — per-cell
  attributes, OSC 8 links, inline images;
- `take_response()` / `response_string()` — DSR/DA/DECRQSS reply bytes.

Every assertion is **semantic** (a cell's text, color, flag, cursor position, or
response byte) — never a pixel. The conformance suite
([`aterm/crates/aterm-conformance/tests/conformance.rs`](aterm/crates/aterm-conformance/tests/conformance.rs))
pins VT/ANSI behavior: plain-text placement, CR/LF/BS, CUP, SGR color and
attribute composition (including the flags-only and color-only fast paths against
the generic path), EL/ED erase, tab stops, autowrap at the right margin, relative
cursor motion, and bottom-row scroll.

This oracle is also fed by a **differential fuzzer** (`tools/conformance/hunt.mjs`)
that compares aterm against xterm.js and auto-minimizes divergences. Its findings
([`tools/conformance/ATERM-FINDINGS.md`](tools/conformance/ATERM-FINDINGS.md)) are
all **grid / cell / cursor** divergences — ED/EL at a pending wrap, wide-char
editing under IRM/ICH, vertical clamping inside a scroll region, wide-glyph
wrapping off the last column, deferred-wrap/LF double-scroll. Each fixed finding
is locked in as a conformance case. Never a pixel divergence: the fuzzer, like the
oracle, reasons about the abstract grid. `resize`-preserves-scrollback is part of
this same semantic contract (the engine carries scrollback across a grid resize,
including the alt-screen cold-restore payload).

### 2. Machine-checked invariants — the `ay` SMT/CHC certificates

A set of specific invariants is discharged by `ay` (the Trust SAT/SMT/CHC solver)
on hand-encoded SMT-LIB2, re-checkable via `bash verify.sh` in each bundle
([`aterm/crates/aterm-spec-models/proofs/ay/`](aterm/crates/aterm-spec-models/proofs/ay/)).
Each bundle follows the `assert_proves_and_catches` discipline: the `unsat`
theorems (negation asserted ⇒ holds for all inputs in the modeled domain) are
paired with `sat` controls proving the encoder is non-vacuous and catches a
deliberately false bound. These are the certificates that exist today:

| Bundle | Subject | Core theorems (all over the stated domain) |
|---|---|---|
| **A1 — `row_index`** | `aterm-grid/.../storage.rs` | `(ring_head + base) % len < len` for all `len ≠ 0`, and no add-overflow on the fast path — the hottest lookup is provably in-bounds (licenses `get_unchecked`). |
| **A2 — base64/hex codec** | `aterm-codec/src/{base64,hex}.rs` | decode is **total** (never panics), the 256-entry / 64-entry table lookups are in-bounds, the `u32` accumulator never overflows, and every encoder output byte is ASCII — licensing `from_utf8_unchecked`. |
| **A5 — coverage-blend** | `aterm-render/src/lib.rs` `blend()` | endpoints are bit-exact (`t∈{0,255}`), and `min(bg,fg) ≤ mix ≤ max(bg,fg)` ⇒ `0 ≤ mix ≤ 255` ⇒ the **unmasked** `<<8`/`<<16` channel packing cannot bleed across channels. |
| **A6 — atlas texture clamp** | `aterm-gpu/src/renderer.rs` | the GPU glyph-atlas height passed to `create_texture`, `tex_h = (h + 256).min(max)`, is always `≤ max_texture_dimension_2d` — an oversized-texture device abort is impossible in scope. |
| **A7 — keyboard shift** | `aterm-types/.../keyboard/encode*.rs` | the legacy Shift map is **effective** (`Shift(c) ≠ c` for every shiftable key) and **total into printable ASCII** — and the bundle's `sat` control catches the historical `to_ascii_uppercase` regression. |
| **A8 — scrollback byte-budget** | scrollback evicting-push | a **CHC** (`HORN`) proof that the budgeted (hot+warm) byte count stays bounded across evicting pushes — the OOM-impossibility / inductive-invariant theorem. |
| **A9 — tab-strip appearance** | `aterm-gui/.../tab_bar.rs`, `aterm-types/.../scheme.rs` | the dark/light partition of the bundled palettes is correct, the dark-path blend factors are **byte-identical** to the pre-appearance code (no-regression), the active card is always distinct, and WCAG `contrast(fg, selection) ≥ 3.0` for every builtin. |

### 3. TLA+ models — concurrency / command-stream discipline

The TLA+ specs under
[`aterm/crates/aterm-spec-models/specs/`](aterm/crates/aterm-spec-models/specs/)
model state-machine invariants, not pixels. The renderer-facing one is
[`specs/GpuEncode.tla`](aterm/crates/aterm-spec-models/specs/GpuEncode.tla): it
models the per-frame **background-instance buffer's create/bind/slice
discipline** — the `NeverSliceEmpty` / `SliceImpliesFill` invariants that the
"buffer slices can not be empty" wgpu panic violated. It is a model of GPU
**command-stream slot discipline** (does an empty frame ever slice an empty
buffer?), **not** of the pixels that come out. The other specs (`AltScreen`,
`ForkExec`, `Sandbox`, `PathConfine`, `WriteAll`) cover alt-screen state, process
spawning, and filesystem confinement — again, all abstract state machines.

---

## What is out of scope, and why

Three things are **categorically out of scope** for the Rust proofs above. The
reason is uniform and structural, not an oversight: **none of them is an input to
any verified artifact.** They live entirely on the TypeScript side of the FFI,
and the engine — by construction — never observes them.

1. **`devicePixelRatio` handling.** The engine is told a single integer pixel
   size via `set_px`. It does not know, and cannot reason about, the
   `devicePixelRatio` that produced that number. DPR is computed and applied by
   the renderer (`fontPx = round(14 × devicePixelRatio)`,
   `aterm-pane-renderer.ts` / `aterm-pane-wiring.ts`); see
   [`aterm-pane-controller-types.ts`](../src/renderer/src/lib/pane-manager/aterm/aterm-pane-controller-types.ts)
   for `ATERM_RENDERER_FONT_PX = 14`. Whether DPR is tracked, applied at the right
   time, and reconciled on a density change is a renderer concern.

2. **Glyph rasterization *quality* at a given device scale.** The Rust visual
   test
   ([`aterm/crates/aterm-render/tests/visual_regression.rs`](aterm/crates/aterm-render/tests/visual_regression.rs))
   does render real pixels and assert **semantic** cell properties — red text
   makes red pixels, a blue-bg cell fills blue, inverse video lightens the
   background, CJK font-fallback draws *something* rather than a blank. Crucially,
   its cell size comes from `rend.cell_size()` — a **font metric** — never from a
   DOM container or `devicePixelRatio`. So the engine proves "the right glyph and
   color are present in the right abstract cell." It does **not** prove that, at
   `devicePixelRatio = 2` on a real retina display, the canvas backing store is
   sized so those glyphs are crisp rather than upscaled-blurry. That end-to-end
   crispness property depends on the device and the DOM, and the A5 README is
   explicit about the analogous GPU gap: the blend proof "says nothing about atlas
   packing, UV/rasterization, the two render passes, or `Rgba8Unorm` readback —
   the device-dependent path."

3. **Renderer↔DOM layout *measurement*.** Computing how many cells fit means
   reading `container.clientWidth` / `clientHeight`, multiplying by DPR, and
   dividing by the device cell metrics (`aterm-grid-size.ts`'s `computeGrid`:
   `cols = floor(container.clientWidth × dpr / cellWidth)`). The **0×0
   zero-dimensions** condition — a pane whose container has no layout yet
   (`display:none`, unmounted, zero-size parent) — is detected and surfaced on the
   renderer side
   ([`pty-connection.ts`](../src/renderer/src/components/terminal-pane/pty-connection.ts):
   *"Terminal has zero dimensions (…×…). The pane container may not be visible."*).
   The engine never measures the DOM and so cannot prove anything about it.

In short: the proofs stop at the FFI because the FFI is where the abstract domain
ends. Below it, the inputs are integers and an abstract grid — provable. Above it,
the inputs are a live browser's DOM and device — not expressible as an SMT/TLA+
obligation, and therefore owned by a different kind of gate.

---

## How the out-of-scope band is covered

Because the device/DOM band cannot be an engine theorem, it is owned by
**TypeScript unit tests around the DOM-measurement seam** and **Playwright e2e
fidelity tests under a forced device scale.** These gates make the end-to-end
crispness/usability property *checkable* even though it is not provable inside the
engine. The seams they pin already exist in the tree:

### Unit gates on the measurement seam

- **`aterm-grid-size` (`computeGrid`).** The seam in
  [`aterm-grid-size.ts`](../src/renderer/src/lib/pane-manager/aterm/aterm-grid-size.ts)
  must (a) **never yield a degenerate 0×0 / 1×1 grid** for a laid-out container —
  it falls back to a usable 80×24 when the container has no dimensions yet and
  clamps to `MIN_GRID_COLS` / `MIN_GRID_ROWS` otherwise — and (b) **scale cols/rows
  with DPR**: the device size is `container.client{Width,Height} × dpr`, so for a
  fixed CSS size a higher DPR yields proportionally more device pixels per axis.
  These are pure-function properties of `computeGrid` and are owned by its unit
  test.

- **`aterm-grid-reflow` (DPR settle).** The reflow seam in
  [`aterm-grid-reflow.ts`](../src/renderer/src/lib/pane-manager/aterm/aterm-grid-reflow.ts)
  must **re-rasterize the engine on a density change**: `applyDpr(nextDpr)` calls
  `term.set_px(round(ATERM_RENDERER_FONT_PX × nextDpr))` and re-reads
  `cell_width` / `cell_height`, then recomputes the grid. The behavior under test
  is a **DPR 1→2 settle**: after it, the engine has been re-rasterized to
  `round(14 × 2)` px and the grid metrics rebuild from the new cell size (rather
  than freezing at the construction DPR and rendering the wrong column count).

### End-to-end fidelity gates (forced `devicePixelRatio = 2`)

Run under a Playwright launch that forces a retina device scale, these assert the
property the engine cannot:

- **Retina crispness** — `tests/e2e/aterm-retina-fidelity.spec.ts`: with
  `devicePixelRatio = 2`, the canvas **backing store equals `round(cssSize ×
  devicePixelRatio)`** on each axis. This is the direct, device-dependent
  crispness check: the backing store is dense enough that glyphs rasterized at the
  scaled font size are not upscaled.

- **Zero-dimensions recovery** — `tests/e2e/aterm-zero-dimensions-recovery.spec.ts`:
  the `0×0` "Terminal has zero dimensions" banner **does not persist** — once the
  pane gains layout, the ResizeObserver-driven reflow re-measures and the terminal
  becomes usable, so the diagnostic is transient rather than a stuck blank pane.

The framing to keep in mind: **the proof boundary is the FFI.** Everything
device- and DOM-dependent is, by design, on the renderer side of it and is owned
by these TS unit + e2e gates. The engine theorems and the device gates are
complementary and **non-overlapping** — keep both. (The A5 and A9 READMEs make the
same point for the GPU path: "keep the lemma *and* the tests.")

---

## The verification boundary, in prose

Picture the data flowing top to bottom, with one horizontal line cutting across
the middle:

```
   PTY bytes ─────────────────────────────────┐
                                               │
   ┌───────────────────────────────────────────────────────────────┐
   │  TypeScript renderer seam   (NOT engine-proven; TS-gated)       │
   │                                                                 │
   │   • measure DOM:  container.clientWidth / clientHeight          │
   │   • read device scale:  devicePixelRatio                        │
   │   • compute grid:  cols = floor(clientWidth × dpr / cellWidth)  │  ← aterm-grid-size unit gate
   │   • scale font:    fontPx = round(14 × devicePixelRatio)        │  ← aterm-grid-reflow unit gate (DPR 1→2 settle)
   │   • guard 0×0:     "Terminal has zero dimensions"               │  ← zero-dimensions-recovery e2e
   │   • crispness:     backingStore == round(cssSize × dpr)         │  ← retina-fidelity e2e (dpr=2)
   └───────────────────────────────────────────────────────────────┘
                                               │
   ══════ THE VERIFICATION BOUNDARY ═ wasm FFI ════════════════════════
        set_px(integer px)   cell_width   cell_height
                                               │
   ┌───────────────────────────────────────────────────────────────┐
   │  Rust engine   (FORMALLY VERIFIED over the abstract domain)     │
   │                                                                 │
   │   • semantics:  bytes in → grid cells / cursor / response out   │  ← aterm-conformance oracle + differential fuzzer
   │   • invariants: A1 row_index · A2 codec · A5 blend ·            │  ← ay SMT/CHC certificates (verify.sh)
   │                 A6 atlas clamp · A7 shift · A8 budget · A9 strip │
   │   • state machines: GpuEncode slot discipline, AltScreen, …     │  ← TLA+ specs
   │   • semantic pixels: right glyph/color in the right cell        │  ← visual_regression.rs (cell size = font metric)
   └───────────────────────────────────────────────────────────────┘
                                               │
   abstract grid / glyphs ─────────────────────┘
```

Everything **below** the boundary takes an already-scaled integer pixel size and
an abstract grid, and is proven or exhaustively tested over that abstract domain —
including a real-pixel render whose cell size is a *font metric*, not a device
measurement. Everything **above** the boundary turns a live browser's DOM and
device scale into that integer and that grid; it is, by construction, not an
engine theorem, and is instead owned by the TS unit + Playwright e2e gates listed
above. The boundary is the wasm FFI: `set_px`, `cell_width`, `cell_height`.
