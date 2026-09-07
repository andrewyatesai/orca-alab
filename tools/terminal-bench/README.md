# terminal-bench — Rust terminal engine vs. xterm.js (head-to-head)

Proves the Orca Rust headless terminal engine (`rust/crates/orca-terminal`,
exposed to Node by the napi addon in `native/orca-node`) is both **faster** than
and **output-identical** to `@xterm/headless`. The Rust engine is what Orca
ships: `src/main/daemon/headless-emulator.ts` loads the napi addon and throws if
it is missing — there is no JS fallback. `@xterm/headless` survives only here,
as this tool's differential baseline/oracle.

## What it measures

The same deterministic ANSI corpus (colored build logs, progress bars with
CR/erase overwrites, 256-color + truecolor runs, attributes, scrolling) is fed
through both engines in identical 4096-byte chunks on a 120×40 grid with 5000
lines of scrollback. We compare:

- **Throughput** — MB/s to fully parse the stream.
- **Parity** — an FNV-1a fingerprint of the final visible grid (trailing
  whitespace normalized identically on both sides). They must match exactly.

## Result (Apple Silicon, 16 MB corpus)

| engine                       | MB/s     | speedup | visible grid |
| ---------------------------- | -------- | ------- | ------------ |
| `@xterm/headless` (baseline) | ~87      | 1.0×    | identical    |
| rust `orca-terminal` (napi)  | ~140–180 | ~1.6–2× | identical    |

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
| `@xterm/headless` (baseline) | ~18  | identical    | identical  |
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

## Agent gate: the gauntlet (run this instead of CI)

`gauntlet.mjs` is the single agent-runnable gate that proves aterm is a full,
superior xterm replacement — no GitHub Actions. It bootstraps its own
prerequisites, then runs three axes and emits a machine-readable verdict:

```sh
pnpm gauntlet                 # all axes; or: node tools/terminal-bench/gauntlet.mjs all
pnpm gauntlet:conformance     # visible-grid differential vs xterm (per ANSI case)
pnpm gauntlet:perf            # MB/s medians + grid-parity
pnpm gauntlet:bootstrap       # install the prerequisites, then verify them
node tools/terminal-bench/gauntlet.mjs bootstrap --verify   # verify only, install nothing
pnpm gauntlet:census          # regret-class ratchet (watched files only shrink)
```

- **bootstrap** — installs the three prerequisites (napi addon, `@xterm/headless`
  oracle, perf corpus) **and then proves them**: the addon is loaded and made to
  parse, the oracle is loaded and matched against the pin in `package.json`, the
  corpus is measured. A path that exists is not a prerequisite — an empty
  `orca_node.node` or a corpus the generator never finished used to read as
  "already present" and exit 0, which is exactly the silently-green failure the
  exit contract exists to kill. Present-but-unusable is a `FAIL`; a toolchain this
  machine simply lacks is `REVIEW`. The checks live in `gauntlet-prereqs.mjs`;
  `--verify` reports the same verdict without installing (no surprise cargo build).
- **conformance** — every case in `tools/aterm-vs-xterm/corpus.json` is run through
  both engines and the 24×80 grids are diffed. A match is parity; a divergence is
  `REVIEW` (not auto-fail), because aterm being _more_ correct than xterm per the
  VT/ECMA-48 spec is a win to triage — see `tools/aterm-vs-xterm/GOAL-B-HANDOFF.md`.
- **perf** — best-of-N medians via `xterm-bench.mjs` / `addon-bench.mjs`, plus a
  grid-parity fingerprint check.
- **safety** — discharges the orca-git SMT obligations with `ay`, resolved via the
  ladder in `rust/crates/orca-git/proofs/ay/resolve-solver.sh` (`$AY` → PATH →
  `~/.cargo/bin/ay` → trust build dirs); `SKIP` (never fail) when the Trust
  toolchain is absent. `verify.sh --solver z3` re-checks the same bundles with
  stock z3 as an independent portability check — ay remains the toolchain of
  record, so the gauntlet itself never substitutes z3.
- **autoformalize** (Goal A) — reuses the Trust repo's `$TRUST_REPO/tools/ts2rust`
  two-witness gate (W1 `trustc -Z trust-verify-full` ∀-safety + W2 Node-TS
  differential). It auto-discovers the already-ported `.ts`/`.rs` pairs under
  `$TRUST_REPO/tools/ts2rust/orca` (`$TRUST_REPO` defaults to `trust` under `$HOME`),
  derives each `fn`+`argspec` from the candidate
  signature, and reports `TRUSTED / NOT-TRUSTED / INCOMPLETE / declined` per function. A
  known-bug port (`*_bug`/`*_naive`) coming back TRUSTED is a soundness `FAIL`; a
  faithful port coming back NOT-TRUSTED is `REVIEW` (port bug vs. a Trust verifier
  precision gap). `SKIP` when the harness or `trustc` is absent. The recorded
  baseline is **397 TRUSTED / 403 kernels / 6 controls refuted**, measured
  2026-07-18 with the toolchain recorded in `autoformalize-ratchet.json`; it is
  historical evidence, not a fresh result from this checkout.
  The ratchet checks both `minTrusted` and `soundnessControls`: a vanished corpus
  or missing controls is `FAIL`; missing, malformed, or incomplete baselines
  cannot pass. With no baseline and no corpus the result is `SKIP`.
  Harness exit status must agree with its unique verdict. Negative controls
  require a static refutation or observed differential divergence; timeouts,
  incomplete proofs, and build errors prove nothing. Production CLI regression
  tests live in `gauntlet-autoformalize.test.mjs`.
- **census** — `tools/repo-census.mjs` regenerates the inventory; the regret class
  (`census-ratchet.json`: the delivery-shim manifest and the watched god objects)
  may only shrink, and growth is `REVIEW`. Triage before you re-baseline: a stale
  ceiling makes the axis REVIEW unconditionally, which detects _nothing_ new. Record
  why the growth was accepted in a `_`-prefixed key (notes, never ceilings) and pull
  the ceiling in whenever a number shrank. **Measure the ceilings at the commit you
  commit them at** — pinning them to an older HEAD lands the axis already red. A
  ceiling cannot be retired quietly: the axis scans the census output, so a deleted
  key, a key hidden under `_`, and a non-numeric ceiling are all `REVIEW`. All of it
  is pinned by `gauntlet-census.test.mjs`, which plants real growth in a watched file.

Exit code is the contract an agent branches on: **0** = every selected gate proved
green, **1** = a real FAIL, **2** = a REVIEW to triage (or a run that skipped some
axis, so it is incomplete), **3** = NOTHING PROVEN — every selected gate skipped.
A SKIP never FAILs (the toolchain may just be absent here) but never reads as green
either, single-gate probes included: `pnpm gauntlet:conformance` without the napi
addon exits 3, not 0. The decision lives in `gauntlet-exit-code.mjs` and is pinned
both ways by `gauntlet-exit-code.test.mjs`. The full report — including the `exit`
code and its one-line `summary` — is written to `.gauntlet-report.json`.

## Files

- `gauntlet.mjs` — the agent gate: bootstrap + conformance + perf + safety, with exit codes.
- `gauntlet-prereqs.mjs` — what "bootstrapped" means: the load/measure checks behind the bootstrap axis (`gauntlet-prereqs.test.mjs` plants the corruption each one catches).
- `run.mjs` — orchestrator: medians + parity verdict.
- `xterm-bench.mjs` — the `@xterm/headless` baseline leg.
- `addon-bench.mjs` — loads the napi addon (`require('orca_node.node')`) and runs the same corpus.
- `snapshot-parity.mjs` — proves the Rust `serializeAnsi()` snapshot replays through xterm to the same visible grid (validates `getSnapshot()` for the renderer).
- `../../rust/crates/orca-terminal/examples/bench.rs` — corpus generator + standalone Rust leg.

## How it's wired into the app

`src/main/daemon/headless-emulator-factory.ts` always builds the aterm-backed
`HeadlessEmulator`; its constructor throws if the native addon is missing (no
flag, no TypeScript fallback — a missing addon is a build/packaging fault).
Node-API is ABI-stable, so the same `.node` loads in both Node and Electron
without an electron-rebuild.

## Live integration test (real Session + real PTY)

`session-live-harness.ts` boots the **actual** `src/main/daemon/Session` against
a real `node-pty` shell under the aterm engine; `verify-live.mjs` checks the
Session snapshot renders identically to xterm parsing the exact bytes the
emulator consumed. `run-scenario.mjs <name>` / `--adhoc '<json>'` drive it.
Verified live: `vim`, `less`, `top`, `python3`, `git log`, colored `ls`,
progress bars, unicode/CJK, 256/truecolor — all byte-identical to xterm.

## Production daemon proof

`daemon-boot-proof.ts` forks the **built, bundled** daemon (`out/main/daemon-entry.js`)
exactly as the Electron app does, then drives it with
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
