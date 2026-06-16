# aterm terminal conformance

A self-contained, **third-party-auditable** proof that the Rust headless terminal
engine (`orca-terminal`, shipped with aterm) renders identically to **xterm.js** —
the reference implementation Orca ships in its renderer.

## Why you can trust it

- **Goldens come from real xterm.js, not us.** `build-corpus.mjs` feeds every case
  through `@xterm/headless` and records the resulting visible grid *and* per-cell
  SGR attributes. Re-run it and you get the same goldens — they are whatever xterm
  actually does, regenerated from source, not hand-written by us.
- **The runner is dumb and total.** `examples/conformance.rs` replays each case
  through the Rust engine and diffs against the golden, byte for byte, exiting
  non-zero on any divergence. No fuzzy matching.
- **Coverage is mapped to xterm's source.** `CHECKLIST.md` lists every handler
  xterm registers in `src/common/InputHandler.ts` with an explicit status
  (TESTED / IMPL / N/A / GAP). Nothing is hidden.

## Audit it yourself (two commands)

```sh
# 1. regenerate cases + goldens from real xterm.js
cd tools/conformance && npm i @xterm/headless && node build-corpus.mjs

# 2. check the Rust engine against them
cd ../../rust && cargo run --release --example conformance -p orca-terminal
#   => "71 / 71 cases match xterm.js"   (exit 0)
```

Tamper with the engine and a case flips to `FAIL` with a row-by-row
`xterm=… rust=…` diff. Add your own case to `build-corpus.mjs` and it is checked
against xterm automatically.

## Files

- `build-corpus.mjs` — case definitions + golden generator (the single source of truth).
- `cases.jsonl` / `goldens.jsonl` — the cases and xterm-rendered goldens (regenerated).
- `corpus.rec` — flat record format the Rust runner consumes (no JSON dep).
- `CHECKLIST.md` — the full xterm handler-registry coverage matrix.
- `../../rust/crates/orca-terminal/examples/conformance.rs` — the runner.

## Differential fuzzing (the real test)

Curated cases prove what we thought of. `fuzz-diff.mjs` proves we didn't miss
anything: it generates large volumes of random-but-structured VT streams from a
weighted grammar, feeds the **same bytes** through both xterm.js and the Rust
engine, and reports any visible-grid divergence — seeded, so every finding
reproduces exactly.

```sh
node fuzz-diff.mjs 20000        # broad grammar, random grid sizes
node focus-fuzz.mjs 50000       # short streams (cursor/scroll/edit/wide/charset)
node bisect.mjs <hex> <c> <r>   # minimize a failing stream to its shortest prefix
```

This loop drove agreement on random streams from ~78% to ~97.5% (broad) /
~99.97% (focused), fixing **ten real bugs** the curated cases missed: IL/DL
cursor-to-column-0, CUU/CUD scroll-margin clamping, DECSC/DECRC full-state
(autowrap/origin/charset) round-trip, pending-wrap resolution for
VPR/HPR/CHT/CBT/ICH/DCH/ECH, VPR screen-edge vs margin clamping, ED-at-pending-wrap,
default vs reverse-wraparound backspace, erase-across-a-wide-char, and VPA
origin-mode relativity. Each is now a unit-test regression.

**The residual ~2.5%** on the broad fuzzer is ~91% origin-mode (`?6h`) + scroll
region + relative-cursor-move combinations where **xterm-6.1.0-beta.220 itself
deviates from ECMA-48** — e.g. CUU moving the cursor *down*, away from the top
margin. On those, the Rust engine follows the spec (CUU stops at the top margin).
No real program emits these pathological sequences; the focused fuzzer (real-world
op mix) is at 99.97%, and all 71 conformance cases + 10 live programs pass.

## Scope

Each case asserts the **visible grid** (glyph placement: cursor moves, scrolling,
erase, insert/delete, autowrap, scroll regions, charsets, wide chars, combining
marks) and, for the `sgr-attr` cases, the **per-cell attribute fingerprint**
(fg/bg colour incl. 256/truecolour, bold/dim/italic/underline/blink/inverse/
conceal/strike/overline). `CHECKLIST.md` documents the handful of rare/legacy
sequences (e.g. selective-erase, GR locking shifts) that are out of scope, and the
reply/title/colour sequences that are intentionally inert in a headless emulator.
