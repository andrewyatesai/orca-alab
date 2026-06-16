# terminal-bench — Rust terminal engine vs. xterm.js (head-to-head)

Proves the Orca Rust headless terminal engine (`rust/crates/orca-terminal`,
exposed to Node by the napi addon in `native/orca-node`) is both **faster** than
and **output-identical** to `@xterm/headless` — the engine Orca currently ships
and runs server-side in `src/main/daemon/headless-emulator.ts`.

## What it measures

The same deterministic ANSI corpus (colored build logs, progress bars with
CR/erase overwrites, 256-color + truecolor runs, attributes, scrolling) is fed
through both engines in identical 4096-byte chunks on a 120×40 grid with 5000
lines of scrollback. We compare:

- **Throughput** — MB/s to fully parse the stream.
- **Parity** — an FNV-1a fingerprint of the final visible grid (trailing
  whitespace normalized identically on both sides). They must match exactly.

## Result (Apple Silicon, 16 MB corpus)

| engine                       | MB/s | speedup | visible grid |
| ---------------------------- | ---- | ------- | ------------ |
| `@xterm/headless` (shipped)  | ~87  | 1.0×    | identical    |
| rust `orca-terminal` (napi)  | ~140–180 | ~1.6–2× | identical |

The **ratio** is the stable metric; absolute MB/s swings with machine load.
Linear bulk text is ~1.6–2× (print-bound); CSI-heavy TUI workloads
(`tui-parity.mjs`) are **~4–5×** (xterm ~18 MB/s vs Rust ~90–140), because the
Rust parser handles cursor positioning / erase far faster.

### Full-screen TUI workload (`node tui-parity.mjs <addon>`)

A realistic TUI stream — alternate screen, absolute cursor positioning (CUP),
erase (ED/EL), 400 animated redraws, and all the snapshot mode flags. This is
what a TUI like Claude Code or vim actually emits.

| engine                       | MB/s | visible grid | mode flags |
| ---------------------------- | ---- | ------------ | ---------- |
| `@xterm/headless` (shipped)  | ~18  | identical    | identical  |
| rust `orca-terminal` (napi)  | ~144 | identical    | identical  |

The Rust engine is ~8× faster here because the workload is CSI-heavy (cursor
positioning / erase), where xterm.js is comparatively slow. Grid **and** mode
flags (alt-screen / bracketed-paste / app-cursor / mouse) match xterm exactly.

## Run it

```sh
# 1) build the Rust napi addon (once)
cd ../../native/orca-node && cargo build --release
cp target/release/liborca_node.dylib orca_node.node    # .so on Linux, .dll on Windows

# 2) generate the shared corpus via the Rust example
cd ../../rust && cargo run -q --release --example bench -p orca-terminal -- gen /tmp/orca-bench/corpus.bin 16

# 3) install the xterm baseline + run the head-to-head
cd ../tools/terminal-bench && npm install
node run.mjs 5 16
```

## Files

- `run.mjs` — orchestrator: medians + parity verdict.
- `xterm-bench.mjs` — the `@xterm/headless` baseline leg.
- `addon-bench.mjs` — loads the napi addon (`require('orca_node.node')`) and runs the same corpus.
- `snapshot-parity.mjs` — proves the Rust `serializeAnsi()` snapshot replays through xterm to the same visible grid (validates `getSnapshot()` for the renderer).
- `../../rust/crates/orca-terminal/examples/bench.rs` — corpus generator + standalone Rust leg.

## How it's wired into the app

`src/main/daemon/headless-emulator-factory.ts` selects the Rust-backed emulator
when `ORCA_RUST_TERMINAL=1` and the addon loads, else the TypeScript
`HeadlessEmulator`. Node-API is ABI-stable, so the same `.node` loads in both
Node and Electron without an electron-rebuild.

## Live integration test (real Session + real PTY)

`session-live-harness.ts` boots the **actual** `src/main/daemon/Session` against
a real `node-pty` shell under the chosen engine; `verify-live.mjs` checks the
Session snapshot renders identically to xterm parsing the exact bytes the
emulator consumed. `run-scenario.mjs <name>` / `--adhoc '<json>'` drive it.
Verified live: `vim`, `less`, `top`, `python3`, `git log`, colored `ls`,
progress bars, unicode/CJK, 256/truecolor — all byte-identical to xterm.

## Production daemon proof

`daemon-boot-proof.ts` forks the **built, bundled** daemon (`out/main/daemon-entry.js`)
exactly as the Electron app does, with `ORCA_RUST_TERMINAL=1`, then drives it with
Orca's own production `DaemonClient` to create a real PTY terminal. It confirms the
daemon logs the Rust-engine selection and the snapshot shows live shell output —
proving the Rust engine runs inside the shipping app's daemon through the full
production build (esbuild bundling, CJS, runtime addon load). Run after
`pnpm install` + `pnpm build:electron-vite`.

## Adversarial swarm

`adversarial-swarm*.mjs` are multi-agent Workflows: each agent crafts adversarial
ANSI for one VT feature, runs it through the live harness, and root-causes any
divergence. Round 1 found and fixed 14 real bugs (deferred wrap, scroll regions,
wide-char columns, charset, tab stops, origin mode, REP, ED3, DECRC-home,
alt-screen re-entry, wide-pair orphaning, …).

## Implemented VT features

Print + SGR (16/256/truecolor, attrs), CR/LF/VT/FF/BS/HT, cursor moves
(CUU/CUD/CUF/CUB/CHA/VPA/CUP/HVP), tab stops (HTS/TBC/CHT/CBT), erase
(ED 0-3/EL 0-2/ECH/ICH/DCH/IL/DL), scroll regions (DECSTBM) + origin mode
(DECOM), alternate screen (1049/1047/1048), DECSC/DECRC + ESC 7/8, IND/RI/NEL/RIS,
deferred autowrap, double-width CJK/emoji (wcwidth-matched to xterm), DEC special
graphics charset (G0/G1, SI/SO), REP, OSC-7 cwd, mouse/paste/app-cursor modes.

**Known limitation:** decomposed combining marks (base + U+0300–036F) — the
single-`char` cell model (and the C ABI's `u32` cell) can't compose multiple
codepoints into one cell as xterm does. Precomposed forms (e.g. U+00E9 `é`) work.
