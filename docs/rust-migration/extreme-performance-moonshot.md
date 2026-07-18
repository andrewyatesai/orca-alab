<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Andrew Yates -->

# The Extreme Performance Moonshot — orc + aterm

**Date:** 2026-07-15 · **Method:** 3 orchestrated workflows, 45 agents (7 codebase deep-dives, 7 research
sweeps, 7 roadmap compositions, 23 adversarial feasibility verdicts — 0 refuted outright, all sharpened).
**Governing constraint:** the product surface stays TypeScript for development velocity; performance and
safety come from the engine, the runtime, and the TS→Trust-verified-Rust factory — never from hand-migrating
product code.

**Number discipline:** every figure below is tagged **[recorded]** (measured in this repo or its docs),
**[external]** (sourced from a third party), or **[target]** (an estimate to be validated by Campaign 0's
instruments before it is ever published).

---

## 0. Thesis: three unfair advantages that compound

Nobody else in the terminal space holds even one of these; orc holds all three.

1. **Own the engine.** aterm: a hand-written ~40-crate Rust workspace with a conformance oracle,
   SMT/CHC proof bundles, and ~776 MiB/s native ASCII parse (~599 CJK / ~340 SGR) [recorded,
   `rust/aterm/README.md:134`] / ~193 MB/s on-glass — 1.5× Alacritty [recorded, `README.md:142`].
2. **Own the compiler between the product language and verified Rust.** Trust (`~/trust`): trustc/tcargo
   ∀-safety proofs, the ts2rust two-witness harness (**243/243 non-control kernels TRUSTED = 100%, 6
   soundness controls refused, 2026-07-18 [recorded]** — see §9), the `ay` SMT/CHC solver, a
   kernel-Certified spine (86/86 [recorded]).
3. **Own the runtime (as far as the evidence justifies).** Electron is itself a ~248-patch overlay on
   Chromium [external] — adding orc patches is a supported workflow, entered rung by rung on profiler
   evidence, never on faith.

**The prize:** the fastest terminal ever built on browser technology — competitive with native record
holders — *and* the first shipping interactive system whose performance and safety claims are
machine-checked theorems a skeptic can re-verify in one command.

---

## 1. The reframing fact: orc is transport-bound, not engine-bound

The full audit of the byte path (PTY → daemon → main → renderer → worker → wasm) found:

| | |
|---|---|
| Engine parse, native | **~776 MiB/s** ascii (~599 CJK / ~340 SGR) [recorded] |
| Engine on-glass | **~193 MB/s** [recorded] — already 1.5× Alacritty's 125 [external] |
| Daemon-leg ingest (aterm headless + pending + fanout) | **161–236 MB/s** [recorded 2026-07-16, `session-ingest-throughput.bench.test.ts`: ascii-log 236, agent-tui 161] — the aterm migration already fixed the OLD 2–15 daemon share; the daemon leg is NOT the bottleneck |
| Daemon→client stream transport | binary frames **1.8–2.8× over NDJSON** end-to-end [recorded 2026-07-16, LANDED as the v1020 binary stream plane] |
| App end-to-end ingest | **"2–15 MB/s" is now STALE** (pre-aterm/old pipeline). Every leg measured 2026-07-16 is far above it — daemon ingest ≥161, transport binary, IPC 1–4 GB/s — so the modern foreground pipe is bounded by the RENDERER OUTPUT SCHEDULER at **~117 MB/s** foreground (`pane-terminal-output-scheduler-throughput.bench`, parse excluded; 1.9 MB/s background = deliberate throttle). **But that 117 is the TIMER-path number** (the bench runs on fake timers): production uses a **MessageChannel drain (interval 0)** — a posted message already yields, Chromium services input/paint between macrotasks (scheduler.ts:1088) — so 117 UNDERSTATES the real path. The scheduler is already sophisticated (budget-capped 8ms max block, MessageChannel drain, coalescing); it is NOT a naive-tuning target, and its real ceiling needs a REAL event loop to bench (fake timers can't). Transport-bound gap Campaign 1 targeted is CLOSED; the path past this ceiling is faster aterm PARSE (upstream aterm work), not scheduler tuning or transport. **PARSE finding 2026-07-16 (measure-first, aterm 7ac4f967, `sample`-profiled, `scratchpad/faster-parse-finding.md`):** the SIMD/v128-scanner assumption is MISDIRECTED — the parser scanner (`advance_simd_loop`) is ALREADY NEON-vectorized (1207 `.16b` codegen ops) and only ~13% of the ASCII-cat hot path. The real throughput ceiling is the CELL-WRITER + scroll: `write_ascii_bulk_fast` scalar run-detection scan **43%**, `write_ascii_blast` per-cell `Cell::from_ascii_fast` writes 9%, scroll/scrollback-extras ~27% (`_platform_memset` is the #1 mixed-workload self-cost). Same trap as reverted 2d/2e (bottleneck ≠ where assumed). Redirected target: vectorize/restructure the cell-writer run-scan via the sanctioned autovectorizable idiom (safe code, stays in the always-on Trust gate like THRU-4a — no ay proof), guarded by the differential oracle + a real `perf_harness` win + byte-identical conformance. **✅ LANDED 2026-07-16** (aterm `d71c2716`, v0.49; orc re-pinned): `write_ascii_bulk_fast` now gates its run-detection scan on a fast vectorized `has_run_of_4` existence check — short OR proven-run-free data blasts directly, skipping the scalar scan. Byte-identical (the no-run scan path already blasts; safe — a false "run-free" only forgoes a splat), bitwise-`&` fold auto-vectorizes (the lazy `&&` measured SLOWER), stays in the Trust gate (no `std::arch`). **+13% ASCII-cat** (~1115 vs ~983 MB/s, `ascii_throughput` example); verified byte-identical: differential oracle 9/0, conformance 194/0, core+grid 2480/0, +3 bulk-vs-per-byte regression tests. The scanner-SIMD (THRU-4b) dead-end avoided by measuring first. |
| `pty:data` IPC payload shape | **REJECTED by the real Electron IPC bench** [recorded 2026-07-16, `electron-ipc-bench/` — a real hidden-BrowserWindow round-trip incl. the C++ message hop]: bytes-payload vs string-payload is **0.96–0.97× on typical ASCII (slightly slower)** and only **1.31× (4KB) / 1.42× (64KB) on control-heavy TUI** — nowhere near the Node `v8.serialize` PROXY's 2.3–22× (`ipc-payload-serialize-bench.mjs`), which overstated it because real Electron IPC is already **1–4 GB/s** and the C++ hop dominates both shapes equally. A wide `pty:data`-contract change (all consumers: renderer, SSH relay, tests) for a ≤1.4× burst-only win that's negative on the common case is not worth it. **Program-shaping consequence**: raw IPC is 1–4 GB/s, so the app's 2–15 MB/s ceiling is NOT the IPC serialization — the bottleneck is the RENDERER-side pipeline (worker wasm parse, batching queues, present path). Redirect ingest work there, not at the IPC payload shape. |
| Current public record (Ghostty nightly) | **~260 MB/s** ingest [external] |

Per chunk today [recorded, data-path dive]: **4 process/thread hops, 3 full terminal parses** (daemon
headless emulator, main headless emulator, worker wasm), **~6 UTF-8⇄UTF-16 transcodes, ~16 buffer
copies, 4 independent batching queues, 4 distinct backpressure systems**. The daemon→client leg no
longer rides NDJSON JSON strings when both ends are the fork's own daemon (v1020 binary stream plane,
landed 2026-07-16 — the in-tree `src/main/daemon/binary-frame.ts` codec is now wired via
`daemon-binary-stream-protocol.ts`). This now covers BOTH client stacks: the main app's
`DaemonClient` AND the coordinator SUBSCRIBER path (`src/shared/daemon-protocol-client.ts` + the
browser-safe `src/shared/daemon-binary-frame.ts` reader, over a byte-migrated coordinator IPC tunnel
— landed 2026-07-16). The Rust daemon already owns PTYs by default on macOS/Linux
(`src/main/daemon/daemon-init.ts:282-291`).

Campaign 1 existed to close the gap between "2–15" and ~300. Update 2026-07-16: that gap was
transport-bound and is now substantially CLOSED — aterm (ingest ≥161, on-glass ~193) + the v1020
binary stream plane on both client stacks removed the parse/copy/NDJSON transport tax, and the real
Electron IPC bench (1–4 GB/s) proved the IPC is not the ceiling. The modern FOREGROUND ceiling is the
renderer output scheduler (~117 MB/s, parse excluded), so the remaining lever is the renderer
scheduler + present path (dirty-band present), not transport. The next honest number to get is a true
modern keystroke/byte→pixel end-to-end (typometer rig), since "2–15" is stale.

---

## 2. The record book (what "winning" means, with sources)

| Axis | Record to beat | orc position |
|---|---|---|
| Ingest throughput | Ghostty nightly ~260 MB/s; kitty 121.8 MB/s ASCII; Casey Muratori's "reasonable floor" 0.5–2 GB/s (termbench/refterm) [external] | engine ~776 native / app 2–15 [recorded] · macOS single-PTY cooked floor ~119 MB/s, pipe ~2.2 GB/s [recorded 2026-07-16] — composite claims gate on min(floor, engine) |
| Typometer latency | xterm 5.3ms · Alacritty 6.9ms · kitty-tuned 10.7ms · Ghostty ~24ms · **VS Code 31.2ms · Hyper 39.8ms** (the Electron incumbents) [external] | unmeasured — Campaign 0 |
| Camera key→photon | foot 15.0 · alacritty 16.7 · kitty 18.3 · ghostty 38.3 [external] | Chromium adds 1–2 vsync stages; sub-10ms camera claims are physically implausible at 60Hz — never publish one |
| Memory | foot 43MB steady [external]; Ghostty 70–90% scrollback compression [external] | engine cell budget 74,020B observed / 96KiB ceiling gate [recorded]; marginal pane 9.1MB [recorded] |
| Wasm engines | ghostty-web: ~400KB, xterm.js-compatible, "not yet optimized" [external] | aterm-wasm must beat it on throughput, latency, and bundle size |
| Verified systems | EverParse: zero-overhead verified parsing in Hyper-V since 2020; seL4: fastest-in-class *and* verified [external] | the bar: verification with **zero performance tax** |

---

## 3. Campaign map

| # | Campaign | Headline win | Effort |
|---|---|---|---|
| 0 | The Record Book | every future claim gets an instrument before an adjective | M |
| 1 | Feed the Beast | end-to-end ingest 2–15 → engine-bound ~300 MB/s [target]; one parse instead of three | L |
| 2 | Photon Discipline | typing frames 19ms → 1–2ms [target]; SSH echo 50–300ms → ~8ms perceived [target] | L |
| 3 | Own the Runtime | SAB now; crossOriginIsolated + wasm threads; the one justified rebuild | S→XL |
| 4 | The Proof Moat | "the parser IS the spec"; memory by theorem; one verified transport | M→XL |
| 5 | The Autoformalization Factory | TS stays, hot paths become certificate-carrying Rust; the paper | M→XL |
| 6 | Native Photons + libaterm | two frontends one daemon; the embeddable proof-carrying engine | XL |

---

## 4. Campaign 0 — The Record Book (measure before touching)

- **Kernel PTY floor per OS**: measure PTY-drain throughput on macOS/Linux/Windows-ConPTY. This is the
  physical ceiling for every ingest claim. **First row recorded 2026-07-16**
  (`tools/benchmarks/pty-floor-bench.mjs`, 5-trial medians): macOS arm64 single-PTY floor
  **~119 MB/s** (cooked mode, ONLCR; plain-pipe baseline ~2.2 GB/s — the tty layer passes ~5% of pipe
  throughput) [recorded]. Consequence: on macOS, single-PTY end-to-end ingest can never exceed ~119
  regardless of engine speed — the ≥260 record attempt needs raw-mode termios, Linux, or multi-stream
  aggregation, and claim №1 is stated as "saturates the kernel PTY floor" wherever the floor binds.
  Pending: raw-mode leg, Linux, ConPTY, daemon-read leg.
- **Key→photon rig, tiered honestly** (per `rust/aterm/docs/FASTER_THAN_GHOSTTY_PLAN.md` tiers).
  **First tier-(c) row recorded 2026-07-16** (`tests/e2e/aterm-echo-latency.spec.ts`, n=120/condition,
  in-process render path): keydown→echo-visible **idle median 6.2ms / p95 9.3 / p99 10.2**; under a
  sustained 256KB/s flood **median 6.3ms / p99 9.9 — load-invariant** [recorded]. Scope: includes pty
  round-trip + transport + parse + first rAF tick; excludes OS input, GPU present, compositor,
  scanout — NOT typometer-comparable. Consequence: the app-internal path already fits in ~6ms, so the
  compositor/present pipeline is the dominant remaining term for the typometer-class claim — exactly
  the rung-1 bench flags + desynchronized-canvas territory. Remaining tiers:
  typometer on all platforms; 240fps camera ground truth only on the pinned M4 Max session; an
  Electron `contentTracing` input-category gate in the **gauntlet** (this repo has no CI by design).
  Keys injected via OS-level events, never PTY-direct.
- **Shipped-blob gate**: today's gauntlet perf leg races the *native napi* build vs `@xterm/headless`.
  Add the wasm-in-worker leg (pass bytes/module to the `--target-web` glue in Node — it has zero
  document/window imports [recorded, verifier]).
- **Ledger discipline**: every published number cites a ledger entry (`rust/aterm/tools/perf-arena/`),
  never a remembered figure. The 5.6× gauntlet ratio and the ~193 MB/s on-glass are the currently
  recorded truths; anything else is [target] until re-run.
- **Generated census**: `tools/repo-census.mjs` regenerates every inventory number (LOC by area, IPC
  channel counts, design tokens, the reliability-shim deletion manifest, largest files) at HEAD —
  docs cite the census, never hand counts. Built 2026-07-15 after an external review caught
  hand-count drift; candidate gauntlet axis.

---

## 5. Campaign 1 — Feed the Beast: one parse, binary bytes, ~4 copies

Verifier-adjusted scope (the "with-changes" versions are the plan of record):

1. **SIMD128 in the shipped wasm** [M]. The build has no `+simd128` today — and the flag alone does
   NOT vectorize the parser: aterm's explicit SIMD paths are x86-64/AArch64 only and wasm takes the
   scalar fallback (`aterm-parser/src/simd.rs:462` [recorded, review-verified]). The deliverable is a
   real wasm v128 scanner family; the flag is its prerequisite (vendored memchr's wasm-simd paths do
   activate for free). Plumb
   `CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS='-C target-feature=+simd128'` (target-scoped) in
   `config/scripts/build-aterm-wasm.mjs` + `--enable-simd` in `WASM_OPT_FEATURES`, both cpu and gpu
   crates. Vectorized plain-text/OSC scanners + ASCII cell blast. Expected 2–4× on scanner-bound
   corpora [target; Photoshop-web measured 3–4× average, byte-loop ceilings 22× external]. Free bonus:
   vendored memchr's dormant wasm-simd paths activate. Proof discipline, Trust-native: lane-abstraction
   trait with the v128 lane-semantics model proven through trust-mc/ay (a v128 lane theory is Trust
   work item T-D below — the host-model gap is a capability to close, not a reason to route around);
   in-wasm differential tests bind the model to the shipped binary until T-B lands and obligations
   derive from the wasm32 MIR itself.
2. **Binary data frames on the Rust daemon protocol** [M]. Version-negotiated (old preserved daemons
   keep NDJSON), control stays NDJSON. Transcodes 6 → **1** (main keeps one decode while side-effect
   authority scanning stays TS; moving those scans daemon-side is a follow-up, not a rider).
3. **utilityProcess socket pump + MessagePort direct to the renderer worker** [L]. Today the daemon
   socket is read on Electron main's JS thread — measured longtasks up to 411ms couple main-process
   stalls into echo latency [recorded]. `MessagePortMain` transfer lists accept only ports, so Node-side
   hops structured-clone: the honest claim is **one copy per process hop** (~16 → ~4), and the
   frozen-main demo applies to daemon-backed sessions.
4. **Byte-based ACK re-base** [L — the largest sub-item, budgeted as such]. UTF-16 chars ≠ bytes:
   pty.ts watermarks/reserves, runtime `sequenceChars`, SSH `seq`/`rawLength`, the renderer ack gate,
   keep-tail caps, e2e hooks all flip atomically. Cross-lane ordering via explicit sequence barriers
   (the `seq`/`startSeq` fields already exist).
5. **Three parses → two** [L] (the honest scope; "parse once" was review-refuted as this item's title).
   Daemon emulator remains the model authority. Main's second headless emulator and the ~8 per-chunk
   JS regex scans (the wait-blocked scan alone measured ~85% of onPtyData cost before throttling
   [recorded]) are replaced by Rust-emitted events from the daemon parse — that is the −33%
   steady-state parse CPU [target] and one full grid+scrollback model per session off the main heap.
   **True one-parse** — the daemon shipping semantic grid/damage deltas plus keyboard-mode, scrollback,
   search, selection, and a11y side-state to a *passive* renderer — is its own XL protocol project:
   it is the coordinator/verified-transport endgame (Wave 4), never a rider on this item.

**Proof of win:** `cat` a gigabyte into a visible pane at `min(kernel PTY floor, ~300 MB/s)` on local
macOS/Linux [target] — above Ghostty's published ~260 [external], from inside Electron. SSH keeps the
legacy path until the relay binding (Campaign 4/6); Windows follows via orca-winpipe or the Node
binary-frame codec at parity.

---

## 6. Campaign 2 — Photon Discipline

1. **Dirty-band CPU present** [M]: typing frames go O(W×H) → O(W×rowH): ~19ms → 1–2ms at 120×40
   [target] — the software fallback fits a 120Hz budget.
2. **GPU path**: grid-in-one-draw-call; WebGPU-in-worker when the Electron pin allows; `desynchronized`
   canvas where the platform engages it (Windows DComp today; macOS needs the unfinished Chromium
   patch — Campaign 3 rung 3) [external].
3. **Verified speculative echo** [L]: **extraction, not invention** — a complete mosh-modelled
   predictor already exists (`rust/aterm/crates/aterm-gui/src/predict.rs`, 559 lines, tested,
   alt-screen-gated, password-epoch-guarded [recorded]). Hoist to a shared wasm-clean crate (web-time
   clock precedent: aterm-effects), expose predict/reconcile/overlay via aterm-wasm, per-keystroke
   renderer→worker message driving the existing `presentNow` fast path. Scope: printables+backspace,
   **remote transports only** (the predictor's own SRTT gate refuses to draw locally), Adaptive default.
   Theorem (provable by construction): *the speculative overlay is display-only; reconcile never mutates
   confirmed grid state* — plus a conformance gate: final grids with speculation ON == OFF.
   Gain: SSH at 50–300ms RTT → ~8ms perceived [target]; mosh's bar is >70% of keystrokes instant
   [external] — match it, then add the theorem mosh never had. Confidentiality caveat (review-found):
   Always-mode can paint unechoed secrets for up to 250ms by design — ship Adaptive-only defaults,
   keep the password-epoch gate, and state the theorem as *display isolation*; grid-equivalence does
   not prove confidentiality.
4. **Cold start**: `orca://` codeCache (V8 measured 20–40% off parse+compile of warm loads [external])
   applied to the ~3.5MB entry; V8 context snapshot swap (no rebuild required; ~1,000ms-class savings
   precedent on apps of this class [external]).

---

## 7. Campaign 3 — Own the Runtime (the ladder; each rung entered on evidence)

**Rung 1 — flags on the stock binary (this week):**
- `--enable-features=SharedArrayBuffer`: SAB byte ring renderer-main→worker with Atomics signaling.
  Scope honestly: SAB agent clusters are per-process, so the main→renderer Mojo copy remains at any
  flag tier; keep a `typeof SharedArrayBuffer` runtime fallback (Chromium deprecation TODOs are still
  active). Append the flag **before** the Linux-E2E early return in
  `src/main/startup/configure-process.ts` [recorded, verifier].
- Flag sweep with correct spellings: `--enable-features=Vulkan,VulkanFromANGLE,DefaultANGLEVulkan`
  (Linux, blocklist may veto), `DelegatedCompositing` default mode (Windows), bench-only
  `--disable-frame-rate-limit`/`--disable-gpu-vsync` for honest latency ceilings. Skia Graphite is
  **already default on Apple Silicon at this pin** — banked, don't double-count [external, verified].

**Rung 2 — the `orca://` migration (header-or-api):**
- Prerequisite discovered by verification: no `orca://` scheme exists; prod loads `file://` and
  Electron deliberately keeps `file://` non-isolated. Migrate serving
  (`registerSchemesAsPrivileged` standard+secure+stream+codeCache, `protocol.handle` over
  `out/renderer`), fix the **two `file://`-hardcoded sender-trust gates**
  (`src/main/ipc/clipboard-ipc-handlers.ts:245`, `src/main/ipc/browser.ts:166`), route feature-wall
  assets, one origin-level localStorage/IndexedDB migration, strict MIME for `.js`/`.wasm`.
- Then COOP/COEP (credentialless) → `crossOriginIsolated`: durable+growable SAB, high-res timers,
  `measureUserAgentSpecificMemory` leak telemetry, **wasm threads** (+1.8–2.9× class on parallelizable
  stages on top of SIMD [external]). **Phase-0 kill-check first:** a scratch privileged window must
  show `crossOriginIsolated === true` on Electron 43 *and* `<webview>` guests must still attach under
  COEP — if not, WebContentsView escape hatch or kill the rung.

**Rung 3 — the one justified rebuild (electron-patch tier):**
- Workload-PGO + BOLT: honest +1–4% [target] — stock Electron 42+ already ships Electron-generated PGO
  (the +9.5% Speedometer / +44–51% contextBridge headlines are that already-shipped change [external];
  do not re-claim them). Linux-only BOLT adds 2–4% [external].
- Pointer-compression-off **variant build** for whale sessions: 8–16GB renderer heap, retires the
  dominant heavy-session crash class; costs up to +40% V8 heap and 5–10% CPU — a variant, not the default.
- macOS low-latency canvas patch (carry what Chromium never finished): 1–2 compositor frames
  (16–33ms @60Hz) off present on macOS [external]. XL maintenance: one patch re-landed every 8-week major.

**Rung 4 — the fork (green-lit 2026-07-15; the ladder is a sequence, not a gate):** `orc-electron` as a
maintained fork. Fork-tier wins to take as engineering items, in value order:
- **Origin-isolation patch**: grant `crossOriginIsolated` to the app's own scheme directly (the PR
  #50789 seam), giving durable SAB + wasm threads everywhere and deleting the `<webview>`-under-COEP
  kill-risk from rung 2.
- **macOS low-latency canvas**: carry the present-path patch Chromium never finished — 1–2 compositor
  frames (16–33ms @60Hz) off keystroke present on macOS [external].
- **Custom Viz terminal surface / delegated compositing where the platform lacks it** — pursued once
  rung-1 bench-mode measurements localize the residual compositor tax.
- **Component stripping + memory posture**: spellcheck/PDF/printing/translate out; PartitionAlloc
  tuning; the pointer-compression-off whale variant becomes a first-class build config.
- **Custom V8 startup snapshot** with the app graph baked in (the Atom precedent, done properly).
- **Routine per-major workload-PGO** (+1–4% honest [target]) once profile collection is scripted.

**The treadmill, run by agents:** Electron carries ~248 patches on its upstreams; majors every 8 weeks;
Chromium moves to 2-week milestones from 153 with weekly security refreshes; local release builds 37min
(M1 Ultra) – 1h49m (M2) per OS×arch; Electron remote-exec is maintainers-only (Postman ported sccache
to MSVC for ~3× cached rebuilds); Discord carries a private fork across ~30 majors; ungoogled-chromium
rides the treadmill with a *mechanical* patch pipeline (317 releases) [all external]. orc's edge: the
re-land cadence is exactly the kind of work this repo already delegates to agents gated by the gauntlet
— patch-rebase waves, per-OS build verification, bisecting breakage. Budget it as standing ops, not an
event. A side product falls out: **making existing Electron apps amazing is itself a portable
capability** — orc-electron (the runtime), libaterm (the engine), and the byte-plane protocol are the
reusable artifacts any Electron app could adopt.

---

## 8. Campaign 4 — The Proof Moat (performance claims as theorems)

1. **Ship the world-first claim NOW — through the owned toolchain** [S]: make the proof artifact
   publicly re-checkable by distributing `ay` (and `ty`) — prebuilt binaries per OS, or a
   `verify.sh --bootstrap` that builds them from the published stage0 seed (the seed pipeline already
   exists and is proven consumable [recorded]). One command re-checks every bundle with the same solver
   that authored it. The "homemade solver" attack is answered by the **Certified rung**, not a second
   solver: ay results reconstruct through the trust-certify CIC kernel (86/86 precedent [recorded]) —
   target: every headline bundle Certified, not merely SmtBacked. The z3 fact is kept as a *property
   statement*: bundles are standard SMT-LIB2/CHC, verified live dischargeable by stock z3 [recorded] —
   portability is evidence of honest encoding, available to any skeptic who wants independent
   confirmation; it is not the plan of record.
2. **The finite-state jackpot** [M]: the VT parser table is 14 states × 256 bytes = **3,584 triples** —
   exhaustively machine-checkable. Build an *independently constructed* spec table from the vt100.net
   DEC machine plus a **machine-checked delta ledger** (colon subparams 0x3A, UTF-8-in-Ground
   diversion, C1 policy — each a named, justified patch); hard-gate every build on every platform with
   a plain generated-table diff (the existing `transition_table_matches_generated` test compares the
   generator to itself — near-vacuous [recorded]). Companion obligations: the dispatch C1-override as a
   second certified patch; SIMD fast paths provably refine the table via the existing equivalence
   harnesses (authored Kani-style, run through trust-mc — the lane that actually executes in this repo).
   Claim: **"the parser IS the spec, modulo N documented, machine-checked extensions."**
   No terminal has ever shipped this.
3. **Resource bounds as theorems** [M→L]: per-tier scrollback bound *RAM(hot+warm) ≤ budget + K_max*
   (extend the A8 bundle; close the all-hot regime); allocation *O(scroll/compression events), not
   O(fed bytes)* — exactly the invariant `perf_scaling.rs` already measures; daemon
   `pending_output ≤ 2MB + overflow-flag` boundedness as a CHC obligation. Flagship demo: the 24h
   agent-churn soak (ARENA-MEM) where competitors show RSS drift and orc shows a flat line **annotated
   with the theorem ID**. Never claim "cannot OOM" — the proof boundary is the wasm FFI and the doc
   that defines it (`rust/PROOF_CARRYING_PERFORMANCE.md`) forbids overclaiming; claim "budget accounting
   is a machine-checked inductive invariant."
4. **One verified transport** [XL]: a single TLA+/model-checked credit-based flow protocol with three
   transport bindings, staged: (a) renderer plane main→worker via MessagePort — this alone deletes the
   char-ACK window, write-off healing, delivery watchdog, and resync probes for ALL providers
   (~1,000+ lines of pty.ts's trickiest code); (b) Rust daemon binary frames (macOS/Linux);
   (c) SSH relay binding. Model two delivery modes — lossless-visible and lossy-background with
   explicit gap markers (preserving keep-tail semantics) — and state the theorem as
   **no-silent-loss / no-wedge / bounded-memory**. Policy pinned (review-forced choice): for an
   unbounded producer with a stalled *visible* client, lossless mode resolves the trilemma by
   **producer backpressure** (block the child at the kernel; never unbounded memory, never silent
   drop); background mode resolves it with explicit gaps. The reliability residue that legitimately
   survives (credits, reconnect generations, snapshot hydration, slow-consumer policy) lives as this
   ONE spec'd mechanism — what dies is the ad-hoc compensator class. The abstract-model→shipped-binding
   refinement gap stays declared until T-G. Ship or document building the `ty` checker for public
   re-checks.
5. **Byte-path safety campaign** [XL]: milestone 1 = get `aterm-parser` through `targo trust check` at
   all (ROADMAP WS-H scope [recorded]). wasm32 width story: build wasm32 std under trustc (T-B) or require
   dual-width (32/64) instantiation of every derived obligation (the A1 bundle's width-uniform theorem
   is the precedent). Two-tier lane: pinned proved-subset ratchet per-commit + nightly full-harness run.
   Honest claim wording: *"panic-, overflow-, and bounds/cast-UB-free (sequential), with licensed unsafe
   sites individually theorem-backed"* — never blanket "UB-free."

### The Trust capability ladder (toolchain investments the moonshot pays for)

Standing rule (2026-07-15): verification and implementation route through the **owned toolchain**
(trustc / ay / ty / trust-mc) first. External tools (z3, Kani, TLC) are validation evidence or
stopgaps, never the plan of record; a Trust capability gap becomes a Trust work item, not a reason to
route around. Each rung below was named by the adversarial verdicts as a prerequisite, and each also
deepens Goal A — capability bought in Trust pays on every campaign; an external dependency pays once.

| Rung | Trust work item | What it unlocks |
|---|---|---|
| T-A | Package/distribute `ay` + `ty` — **assessed 2026-07-16 [recorded]**: both binaries fully standalone (system libs only, verified under `env -i` from `/`), Apache-2.0 with LICENSE/NOTICE/THIRD_PARTY, byte-copied `ay` discharged 4 real orca-git obligations. Design: `ay-<ver>-<triple>.tar.zst` (binary+licenses+manifest w/ build.commit) + SHA256SUMS; orc pins hashes fail-closed; `verify.sh --bootstrap` fetches→verifies→caches→exports AY into the ladder; linux musl static via ay's existing `build_linux_static.sh`; stock-cargo-at-pinned-rev as the documented fallback rung (minutes, vs the private 479MB macOS-only seed). **Blocker = publication decision (Andrew): flip the ay public mirror (release machinery exists) vs interim orc release assets.** Ship `ty` when a bundle consumes it. | public one-command re-check; paper artifact evaluation |
| T-B | wasm32-unknown-unknown std under trustc (`-Zbuild-std` class) | obligations derived from the **shipped blob's** MIR — "the artifact you run is the MIR we proved"; licenses guard deletion in wasm |
| T-C | Certified-by-default: headline bundles reconstruct through trust-certify (§7.4 typed-equality reconstruction where needed) | the homemade-solver defense; the Certified rung as the public face |
| T-D | v128/SIMD lane theory + trust-mc modeling gaps (MaybeUninit, Vec, multi-variant enums) | verified SIMD scanners beyond scalar models (Campaign 1) |
| T-E | Interprocedural equality: wire whole_program.rs callee summaries; lockstep/relational invariants for loop-bearing pairs | proven-equivalent kernels (Campaign 5 T1) |
| T-F | Solver frontier: overflow-interval propagation, uninterpreted external-call havoc, native CHC/PDR evidence emission | the byte-path zero-FAILED campaign (item 5 above) |
| T-G | Temporal lane (ty-bridged liveness with kernel recheck) | transport no-wedge as true liveness; P3's "every gate reopens" |

---

## 9. Campaign 5 — The Autoformalization Factory (the paper nobody else can write)

**✅ CENSUS HIT 100% — 2026-07-18 (`pnpm gauntlet:autoformalize`): 243/249 TRUSTED = ALL 243 non-control kernels W1-proved (trustc ∀-safety) AND W2-0-divergence; the 6 `_bug`/`_naive` soundness controls stay refused; 0 declined, 0 faithful-miss, 0 breaks.** This SUPERSEDES the "54 TRUSTED / 165 faithful-misses" census below and, importantly, **corrects that section's root-cause diagnosis with measured reality.** The 165 reservoir did NOT close via the predicted heavy toolchain investment (nonlinear/division theory + loop-invariant *synthesis*). It closed via four narrower, higher-leverage fixes — each dissolved a whole obligation class — plus reformulation:
1. **A trait-callee matching bug** (`trust-ir-bridge/lower.rs`): `strip_generics` deleted the `<Recv as Trait>` qualifier, so trait-dispatched spellings (`<Chars as Iterator>::map`, `<String as PartialEq<str>>::ne`, `ToString::to_string`, `From::from`, `Deref::deref`) collapsed to a bare method name that NEVER matched the flat panic-free allowlist — the whole lazy-adapter/alloc-conversion/Deref frontier was silently dead. Added `TRUSTED_PANIC_FREE_TRAIT_TAILS` (SOUND: eager consumers `all`/`any`/`find`/`collect`/`fold`, `count`/`sum`/`product`, and `Index` EXCLUDED — the soundgate caught the unsound version, which dropped the Undef-result cast obligation). **101 → 175.**
2. **The +415-solver-commit rebuild** of latest trust (rustc `1.99.0-dev 4c9dc90f4`) — `is_panic_free_drop_ext` now recognizes String/Vec/Option/Result/MaybeUninit drop-glue as panic-free, organically dissolving the 115-kernel `unsupported-mir-drop` class. **90 → 101.**
3. **Differential-harness fixes** (the W2 side was silently failing correct ports): the census fuzzed struct fields as bare names → all `u32` → serde rejected `bool`/`str`/`Option` fields (`invalid type`) and camelCase structs (`missing field`); `verify.mjs` maxBuffer was Node's 1 MiB default → large-output kernels ENOBUFS'd; and a TS `field?` optional must be fuzzed as an **omitted key** (serde `None` = TS `undefined`), not JSON `null` (`null !== undefined` is true in TS). **~+17 struct kernels + the last 2 optional kernels.**
4. **~78 diagnostic-first reformulations** (the `reform-v2` workflow: each agent runs the driver, reads the EXACT obligation, applies the matching behavior-preserving pattern). Crucially, **many "loop/counter" kernels the diagnosis flagged for loop-invariant synthesis were instead rewritten to saturating byte-scans / factored sums** that carry NO loop-invariant or overflow obligation at all — sidestepping the predicted gap rather than paying it down. **175 → 243.**

**Consequence for the factory:** the reservoir the section below theorizes about is now a PROVEN 243-kernel equivalence corpus (W1 ∀-safety ∧ W2 0-divergence), and the honest lever was **matching/allowlist correctness + harness fidelity + reformulation**, not (primarily) nonlinear/loop-invariant theory (those remain real T-D/T-E investments for the harder shipped ports, but were NOT the bottleneck for the corpus). Ratchet floor `tools/terminal-bench/autoformalize-ratchet.json` = 243. Raw material for F4/P-ports is ready; the open work is **promotion** (proven kernel → shipped napi/wasm shadow), not proving. Historical diagnosis retained below for provenance.

**The audit's key discovery:** two tracks exist and have never been fused. Track 1 (shipped): LLM-agent
hand-ports with verbatim test translation, parity corpora (1,406 dispatch-parity cases — measured), ay safety bundles, and a proven
promotion recipe (parity → napi/wasm via orca-dispatch → shadow cutover → delete the TS twin — orca-git
landed this way: 137 tests, 10 SMT obligations, TS deleted 2026-07-06). Track 2 (unpromoted): the
ts2rust two-witness autoformalizer — W1 `trustc` ∀-safety, W2 Node-TS differential fuzzing.
**GATE-MEASURED, FULL CENSUS RUN 2026-07-16** (`pnpm gauntlet:autoformalize`, warm shared cargo target
→ ~8 min, not the feared hours): **54 TRUSTED / 218 auto-runnable** (243 corpus pairs − 22 arg-declined
− 3 skipped; +1 from a compare-form flip after the run). **This SUPERSEDES the long-tracked "146–151/208"
— that number never matched the reproducible gate** (it counted a broader hand-tally / W2-equivalence, not
strict W1∧W2 TRUSTED). Ground truth: **0 soundness-breaks** (no `_bug` kernel passed) and **165
faithful-misses** — ports that are W2-*equivalent* (0 divergences) but W1-INCOMPLETE. Those 165 are the
real reservoir: a small pure-predicate slice is recipe-fixable (range-contains → compare-form flipped
`isEcmaTrimWhitespace` + `isDecorativeTitleWhitespace`), but MOST share **ONE upstream root cause, traced
to the exact code 2026-07-16**. trustc lowers each safety obligation to a typed-CHC by DROPPING
"un-lowerable hypothesis conjuncts" (`trust_verify.rs:15704-15719` violation-pruning; the free var then
surfaces as `ay-chc::smt::check_sat`'s unassigned `__fact_`/`_undef_` "in an evaluable theory position").
`P UNSAT ⇒ original UNSAT` keeps proofs sound, but the pruned `P` is now UNDER-constrained, so DPLL(T)
finds a SPURIOUS counterexample and check_sat *correctly* fails closed to Unknown → NOT-TRUSTED (accepting
was a verified fail-OPEN). The dropped hypotheses are exactly the **division relation** (`q = d/2`) and the
**loop-summary invariant** (`n += 1` counters) — so the division kernels AND the counter/loop kernels are
the SAME gap. Fixing it is NOT a rewrite and NOT a check_sat patch (both verified dead-ends): it needs the
two capabilities that would make those hypotheses lowerable — **nonlinear/division theory** and
**loop-invariant handling**. **REFINEMENTS 2026-07-16, honest about the mix — the 165 is NOT uniformly a
"precision gap":** (a) trustc already HAS loop-invariant *checking* syntax (`while cond invariant P
decreases e`, first-class HIR, verified in `rustc_ast`/`rustc_parse`) — so it is *synthesis* + speed that
are missing, not the check; BUT loop verification is SLOW — a minimal bounded counter loop TIMED OUT at
2 min under trustc, so many loop kernels likely hit the driver's 180s cap (a TIMEOUT NOT-TRUSTED, not a
clean precision-fail). And the `invariant` keyword is a trustc extension that won't parse under the
stock-rustc W2 build, so using it needs a harness change (W2-via-trustc). (b) W2-equivalent ≠ faithful at the
extremes: `countWhitespace -> u32` overflows past 4.29 B chars where TS `number` (f64) would not, and W2's
bounded fuzz can't reach it — so trustc's W1 rejection is CORRECT there, not a false miss. **But the
narrow-int overflow AUDIT (ran it 2026-07-16) came back essentially clean:** across the whole corpus, the
ONLY genuine non-saturating narrow accumulator is that `countWhitespace -> u32` (and it already has a u64
twin, `countws_fix`); every other narrow-return `+`/`*` kernel is either a `_naive` bug-control or
provably bounded and TRUSTED (trustc proves `min(depth,6)*10+10`). So real narrowing bugs are ~1,
negligible — the 165 are OVERWHELMINGLY genuine verifier limitations, NOT port bugs (this *strengthens*
the faithful-miss reservoir claim and corrects my own earlier overcaution). Net: the reservoir is
loop-timeouts + true precision gaps; the honest lever is loop-invariant *synthesis + verification perf*
and nonlinear/bounds theory — a narrow-int audit is NOT needed (done, clean). That is the true Goal A ceiling: the 54 TRUSTED are the straight-line
linear no-loop kernels; the 165 are dominated by loops/division awaiting those capabilities.
**QUANTIFIED (reliable loop-detection over comment/string-stripped source): 104 of the 165 (~63%) contain
a loop/iterator → LOOP-INVARIANT SYNTHESIS is the single highest-ROI trustc investment; the other 61 are
straight-line (string/char index-bounds + nonlinear/division).** So the capability spend is rank-ordered:
invariant synthesis first (unblocks ~104), then nonlinear/bounds theory (~61). Outputs sit
in `~/trust/tools/ts2rust/orca`, never shipped. The factory = fuse them.
  **⚠️ CORRECTION (later 2026-07-16) — "54 is the ceiling / the 61 straight-line are index-bounds +
  nonlinear/division" was PARTLY WRONG.** Per-obligation probing (`trustc -Z trust-verify-output=json`)
  of the simplest straight-line misses showed a THIRD blocker class the loop/division framing missed:
  **allocation.** A kernel whose value is a borrowed substring or a string literal but that calls
  `to_string()`/`String::from()`/`collect()` trips an absent-callee `may panic` assumption (the callee
  body isn't in trustc's lowered bundle) — a *formulation* issue, not a solver gap. Returning the borrowed
  `&str` (via the lowered `strip_*`/`trim`/`split_once`) or a `&'static str` literal clears it with zero
  allocation. Recovered the census 54→72: **+9 discovery-coverage** (a mechanical decline bug, see F4
  section) and **+9 allocation-gated** rewrites. So the ceiling was never 54, and the reservoir is a MIX of
  THREE classes: allocation-formulation (bounded-recoverable, now mostly harvested — last scan 1/5),
  loop-invariant-synthesis (~the 104), and nonlinear/division (the residue). Only the last two are genuinely
  research-scope. The rank-ordering above still holds for the *solver* residue.
  **↳ VERIFIED ROOT CAUSE of the absent-callee wall + why the residue is owner-gated (read the trustc
  source, 2026-07-16):** the "absent callee may panic" obligation is emitted by a DELIBERATE,
  soundness-critical, **ABI-gated fail-closed** boundary (`trust_verify.rs:10473-10516`,
  `extern_abi_is_non_unwinding` — comment: "The soundness-critical ABI whitelist … stay fail-closed").
  trustc discharges an out-of-bundle call's panic-freedom ONLY when the callee's ABI is a non-unwinding
  C-family boundary (a panic there ABORTS, can't unwind into the caller) or it is `#[trust::skip]`'d. Std
  methods like `to_string`/`chars`/`collect` are `extern "Rust"` with bodies outside the lowered bundle,
  so trustc stays fail-closed — it cannot see that they don't panic. **CORRECTION 2026-07-17 (owner
  called out the earlier gloss):** default-Rust allocation failure ABORTS (`handle_alloc_error`), it does
  not unwind, and `str::to_string` has essentially no other panic path — so the earlier "CAN unwind —
  OOM-panics" parenthetical was wrong, and the earlier "axiomatizing changes what TRUSTED guarantees"
  framing was overheated: trusted std specs are STANDARD verifier practice (Verus/Creusot/Kani all ship
  them); the real questions are TCB documentation + which spec mechanism. Probed 2026-07-17:
  `#[trust::skip]` exists (tool attribute on a locally-compiled callee; a `#[trust::skip]` wrapper around
  `to_string` demotes the caller's VC to `[trust-expected-absent-callee-assumption]`) but is
  **advisory-mode only** — under the driver's strict W1 it fail-closed ABORTS (`UserOptOut` rejected;
  "every Level 0 obligation must be statically proved"). So the builder class needs either a documented
  conditional-TRUSTED tier (lame-mode W1 with the assumption ledger) or a strict-compatible trusted-spec
  path — likely the E2/E9 `ensures`-discharge machinery landed upstream 2026-07-17 (+415 commits pulled,
  stage2 rebuilding). Later loop diagnosis (`f853ee7b5`): the ~104 "loop kernels" are ALSO not uniformly
  invariant-gated — a byte-scan (`as_bytes()`+`.get()`+saturating) clears the ASCII-single-code subclass
  in ~2s with no invariant (recovered countLinesEmptyAsZero, 73). Net census **54→73**.
  **↳ REPRODUCIBLE blocker breakdown (`pnpm blocker-census`, `tools/autoformalize-blocker-census.mjs`,
  2026-07-16):** the manual per-kernel analysis above is now a machine-run classifier — it re-runs each
  kernel's W1 (the driver's serde-stripped lib wrap) and buckets every UNPROVEN obligation. Corpus at the
  start of the harvest: 249 · 75 W1-verified · 174 incomplete · 24 formulation-recoverable · 150 residue.
  **After the harvest (re-run 2026-07-17): 249 kernels · 89 W1-verified · 160 W1-incomplete · 10
  formulation-recoverable · 150 solver-residue** — 14 of the 24 recoverable candidates were harvested
  (recoverable 24→10), and the residue is now cleanly dominated by owned-String builders. By blocker class
  (kernels carrying it): `unsupported-mir-drop` 115 (owned-String builders — the residue), `absent-callee-
  other` 38, `absent-callee-iter` 35, `unsupported-mir-arith` 28, `absent-callee-alloc` 19, `timeout` 12,
  `unsupported-mir-bounds` 12, `other` 11. **CORRECTION 2026-07-17 (probed after the owner pushed back):
  the "faithful subset is smaller — non-ASCII/NFA/nested scans are NOT recoverable" claim was a FALSE
  UNIVERSAL, refuted 5/5 by actually probing.** `str::get(range)` generates ZERO obligations (Option
  return — now on the known-LOWERED list), unlocking slice-returning rewrites; and UTF-8 is deterministic,
  so byte patterns exactly match specific non-ASCII sequences (braille U+2800–28FF = `E2 A0..A3 80..BF`;
  `"π - "` = `CF 80 20 2D 20`). All five previously-written-off kernels probe TRUSTED:
  command_token_basename (backward scan + `get(idx+1..)`), b5_tablesep (indexed cells + `get(range)` +
  lowered trim), contains_braille + is_pi_agent_title (byte patterns), cap_opencode_text (lead-byte scalar
  count + `get(..idx)`, String→&str). Pending re-verification under the rebuilt stage2 toolchain before
  adoption (the census will move 90→95). The residue split is a reproducible number; the 115 owned-String
  builders remain the spec-gated class (see the corrected note above).
  **✅ E1 → Goal A cross-connection 2026-07-16** (`~/trust` `86bc1b56f`, `108f9f753`): this session's E1
  decision cores are prime autoformalize candidates. Added **+5 TRUSTED kernels** derived straight from
  landed E1 units, spanning 4 of the 6 E1 crates — each W1 `trustc` VERIFIED + W2 0 divergences:
  `e1_expiredir` (`shouldExpireSessionDir`, orca-session-gc, 0/26), `e1_backoff`
  (`activeFailureRefetchThrottleMs`, orca-provider-backoff, 0/18), `e1_gpufallback`
  (`shouldEngageGpuFallback`, orca-crash-recovery, 0/80), `e1_flowaction` (`producerFlowAction` — the
  flagship PtyProducerFlowController hysteresis decision, orca-flow-control, 0/35), `e1_recoveryallowed`
  (`isRendererRecoveryAllowed`, orca-crash-recovery, 0/83). **FINDING:** a keep-tail kernel is
  W2-equivalent (0/18) but stays NOT-TRUSTED on a division `ArithmeticSafety` obligation.
  **CHARACTERIZED PRECISELY 2026-07-16** (minimal repro `div_guarded`/`div_checked`/`div_clamped`, all
  u64): this is NOT "trustc can't verify division" — the **native `trust-mc` full verifier PROVES it**
  (`requested=2, proved=2, failed=0`). The gap is a *reconciliation/encoding* one: the `ay_chc` SMT path
  models the division result as an `_undef_` free variable for the div-by-zero case, and
  `ay-chc::smt::check_sat` returns **Unknown** ("SAT model … missing an assignment for free variable
  `…_undef_0` in an evaluable theory position; default-value completion … did not strictly verify"), so
  strict mode takes the worse verdict and rejects the native proof as "lacked exact kernel/native proof
  authority." It fires on EVERY form — raw-guarded (`if d==0`), `checked_div().unwrap_or`, AND
  divide-by-`max(1,_)` — so there is **no recipe workaround**. **DEEPER 2026-07-16** (read
  `trust_verify.rs:16344-16382`): the native "Proved" is downgraded to `unknown` by a *deliberate
  fail-closed soundness guard* — a `Proved` result counts ONLY if the exact-proof `authority` object
  `permits_static_proved_transport_for(index,vc,binding)`; a solver's/native's bare Proved LABEL
  intentionally carries no authority ("missing/misaligned exact authority fails closed"). So the tempting
  shortcut — "just accept the native trust-mc proof" — is *architecturally forbidden*: the sound fix MUST
  make `ay-chc` actually discharge the division VC so it mints the authority artifact, i.e. fix the
  division `_undef_` VC encoding / guard-propagation in `first-party/ay/crates/ay-chc/src/smt` (NOT a flag
  in `trust_verify.rs`). Verified genuine SMT/encoding work + slow stage2 rebuild — out-of-session, but now
  *actionable to the exact function + confirmed no shortcut exists*, not vague. So the E1 cores that TRUST today are the compare-only /
  saturating-total-ops ones; division kernels (keep-tail), f64 (renderer-heap) and `&[u16]` (stream-split)
  cores wait on that capability. Recipe to advance cheaply meanwhile: mine landed E1 pure cores, prefer
  saturating/compare/total-ops formulations (NOT `checked_div` — it hits the same wall).
  **✅ Non-E1 kernels 2026-07-16** (`~/trust` `97a1c02e9`, `3060c5edc`, `1d3582c34`): the recipe
  generalizes past the E1 cores — **+5 TRUSTED** mined straight from production orca src with no E1
  backing: `b_tocmaxwidth` (`computeMaxMarkdownTocPanelWidth`, markdown-toc-panel-width.ts — the lone
  subtraction widened to i64 so trustc discharges it, 0/13), `b_prechecktimeout`
  (`normalizeAutomationPrecheckTimeoutSeconds`, automation-precheck.ts, 0/13), `b_recoverablesignals`
  (`aiVaultSessionRecoverableSignalCount`, ai-vault-types.ts — a new *clamped-sum* family, 0/59),
  `b_workercount` (`boundedWorkerCount`, map-with-concurrency.ts — a *bounded-fan-out* transport-safety
  clamp, 0/59), `b_termcols` (`clampTerminalCols`, orca-runtime.ts — PTY dimension bound, 0/13).
  **Session total: 10 new TRUSTED kernels** (5 E1-derived + 5 mined), each W1 VERIFIED + W2 0-divergence.
  **✅ Auto-census coverage widened 2026-07-16** (`0754d20f3`): the `autoformalize` gate DECLINED every
  kernel with a `u16`/`i16` arg (real orc types the Trust fuzzer already models) — adding them recovered
  **9** already-ported decision cores whose only blocker was the arg type (~209→218 gate-runnable, the
  measured count), all verified TRUSTED (`computeRestoreTargetLine`, `isAtBottom`, workspace-name whitespace/word-sep
  predicates, `toLowerAsciiCode`, …); one (`isEcmaTrimWhitespace`) was `RangeInclusive::contains`
  (trustc-runtime-checked) → rewritten to compare form for W1 (`~/trust` `<local>`). A bare-tuple-return
  guard (`-> (i32,i32)`) declines the 2 kernels W2 can't diff, so no REVIEW noise.

- **✅ F1 Provenance gate** [M] — **DONE, and now covers the E1 tier** (`93d835a4e`): every TS→Rust port
  pinned to its source hash (`tools/port-provenance.mjs` → `port-provenance.json`, enforced by the
  `provenance` gauntlet axis); upstream TS drift fails LOUDLY with a structured re-port task instead of
  being caught reactively by parity mid-merge (the last upstream merge caught 5 shadow-port drifts
  *reactively*). This session closed the gap where the 8 E1 decision cores — shared-corpus ports with no
  orca-dispatch adapter — were certificate-enforced but NOT source-drift-gated: the ledger deriver's
  crate-name regex silently dropped every hyphenated crate, so broadening it + adding an E1 ledger
  section pinned all 8 (103→111 modules, 210→226 files). A `PROVENANCE_ROOT`-overlay drift self-test
  confirms an E1 TS edit now fires the re-port task. E1 units now carry the FULL regression-gated
  contract: ay certificate + parity corpus + source-drift pin.
- **F2 Trace-derived corpora** [M] — **metrics half DONE 2026-07-16** (`tools/parity-corpus-metrics.mjs`,
  `pnpm corpus:metrics`): mechanically discovers BOTH corpus families (dispatch-parity `vectors/*.json`
  + E1 shared `*parity-corpus.txt` — the ad-hoc globs it replaces silently missed the plain
  `parity-corpus.txt` names) and publishes the Cedar-style headline: **1,510 machine-checked
  (input→expected-output) behavioral parity cases across 87 modules** (1,406 dispatch-parity / 79 modules
  + 104 E1 / 8 corpora / 6 crates), each re-run every gauntlet pass. **And GATED** (`gauntlet-corpus.mjs`,
  new `corpus` axis in `all`, `pnpm gauntlet:corpus` / `corpus:check`): the count may only GROW —
  `parity-corpus-baseline.json` ratchets it, a drop FAILs (a deleted/shrunk corpus is a regression in the
  equivalence net). Remaining F2 half: auto-*record* (input,output) pairs at the orca_dispatch seam +
  from vitest runs to GROW the corpora unattended.
- **F3 Real TS front-end** [L]: vendor swc/oxc — inferred argspecs, auto-extracted oracles, generated
  Rust skeletons; agents only fill `todo!()` bodies. Target: an order-of-magnitude drop in
  agent-minutes-per-TRUSTED-pair.
  - **Discovery-coverage fix LANDED 2026-07-16 (602515a19):** the census's Rust-signature→argspec
    inference (the argspec half of F3, already in `gauntlet.mjs`) was silently declining/skipping 25
    already-ported kernels for three MECHANICAL reasons — a trailing comma in a multi-line signature
    splitting into a phantom empty param (22), a generic/lifetime clause (`fn f<'a>(…)`) breaking the
    sig regex, and lifetime annotations (`&'a str`) missing the argspec table — plus a needless
    bare-tuple-return skip (a tuple serializes to one JSON array, so W2 diffs it fine). Fixing all four:
    **63 TRUSTED / 243 runnable / 0 declined** (was 54/218), ratchet floor 54→63. So argspec inference
    now covers 100% of the ported corpus; the remaining F3 `[L]` is the swc/oxc auto-EXTRACTION of NEW
    kernels + skeleton generation, not the argspec derivation.
- **F4 Close the loop** [L]: unattended classify→port→verify→promote for in-fragment kernels **whose
  signature matches the live export** (the verifier's key restriction — TRUSTED kernels with narrowed
  types can't ship as-is); promotion re-runs autoformalize against the real module source; ships
  through the existing one-export orca-dispatch seam. Inventory honesty (full census 2026-07-16, after
  the discovery-coverage fix + soundness-control twins + the allocation-recovery pass): **90 TRUSTED /
  249 runnable / 0 declined / 6 controls refuted / 0 soundness breaks** — the numerator moved 54→90 by
  recovering kernels, NOT re-tallying; `pnpm gauntlet:autoformalize` is the numerator, not a hand-count.
  The 6 `_bug`/`_naive` controls now have correct `.ts` twins, so they RUN and are all rejected — W1
  catches the unchecked cast/add/u64-accumulate (countws/fontsize/csiparams), W2 the semantic divergence
  (packrgb/unpackrgb/sumpos) — so "0 soundness breaks" now means every bad port was caught. **Correction
  to the faithful-miss diagnosis:** they are a MIX, not uniformly loop/division-gated. A distinct
  recoverable class is ALLOCATION-gated — a substring-returning kernel calling `to_string()`/`collect()`
  trips an absent-callee "may panic" assumption (the callee body isn't in trustc's lowered bundle);
  returning the borrowed `&str` (lowered `strip_*`/`trim`/`split_once`) or a `&'static str` literal clears
  it with no allocation — recovered 8: stripGitSuffix/optionName/trimRuntimePathTrailingSlash/
  stripGrokUserQueryWrapper (`&str`) and getLocalExecutionHostLabel/getOrcaCliCommandNameForPlatform/
  mobileDiffLinePrefix/parseStatusChar (`&'static str`). NOT recoverable this way: string builders
  (push/collect/char-Vec + loop), owned-struct-param borrows, and fn-pointer/closure predicates
  (`rsplit(|c|)`/`trim_matches(fn)` are themselves absent callees). The residue is genuinely
  loop-invariant-synthesis / division-VC gated (`ay-chc/src/smt`, research-scope). (The census depends on
  the local `~/trust/tools/ts2rust` corpus, not reproducible from the orc repo alone.)
  - **First seam promotion LANDED 2026-07-16 (63e53d894):** the E1-certified,
    autoformalize-TRUSTED `orca-provider-backoff` core (capped-exponential refetch throttle) now flows
    through the production orca-dispatch registry as parity module `provider-backoff` — live TS adapter
    vs Rust twin over 11 vectors, TS==Rust==golden, dispatch-missing 0 (parity now 1417 cases / golden
    1410). **Additive only:** the shipping throttle stays TS; no hot-path call site is cut over (the
    manifest's no-cutover-without-a-same-day-bench-win rule holds — the seam wiring carries no perf
    risk, the cutover is a separate, owner-gated decision). Both factory gates absorbed it: F1
    provenance pins TS↔Rust twin (and gained a general fix — the resolver now maps single-file crates
    that define their API directly in `lib.rs`, the shape of every small E1 core, so future promotions
    resolve cleanly); F2 corpus ratchet 1510→1521. This is the end-to-end proof the promote leg works;
    the remaining `[L]` is the *unattended* classify→port→verify→promote automation, not the seam.
  - **Generalized by a second promotion 2026-07-16 (f2e83a4c6):** `orca-flow-control::keep_tail` — a u64
    **division+clamp** core (`clamp(BUDGET / max(1,n), [MIN,MAX])`) + a `keepTail*2` drop cap, 2
    functions — went through the seam the same way (parity 1431 / golden 1424, corpus 1521→1535). So the
    promote leg now holds across two crates, three functions, and two distinct kernel classes
    (shift/saturate + division/clamp) — evidence the pattern is general, not fitted to one lucky kernel.
- **Port targets by measured heat:** P1 the onPtyData chunk-ingest core as one Rust scan pass
  (**UTF-16 code-unit seam mandatory** — napi string conversion replaces lone surrogates and PTY chunks
  split astral pairs; re-baseline heat on current main first). P2 — **re-scoped by measurement
  2026-07-16**: the napi-string `RustNdjsonParser` cutover was implemented, proven wire-identical
  (parity green), benched, and REJECTED — ~30% slower end-to-end than the TS parser (458 vs 657 MB/s
  full pipeline; split-only 810 MB/s vs 4.5 GB/s) because per-line UTF-16⇄UTF-8 FFI copies dominate
  while V8 substrings are copy-free [recorded]. The old parity-test comment predicted exactly this;
  the bench gate held. P2 is therefore **binary frames with Buffer payloads only** (near-zero-copy
  napi externals) — string-shaped FFI on hot paths is a proven dead end; the manifest rule: no Rust
  cutover on a hot path without a same-day bench win. **P2 LANDED 2026-07-16** as the daemon→client
  **v1020 binary stream plane** (opt-in `streamFormat:'binary'` on the stream hello; NDJSON stays the
  negotiated default, so a non-granting daemon keeps both ends on NDJSON by construction). PTY output
  rides as raw `[type:u8][len:u32BE][sidLen:u8][sessionId][raw bytes]` frames — no per-chunk
  `JSON.stringify`/`parse`, no `\uXXXX` control-byte expansion; non-data events ride as their
  NDJSON-identical JSON text in an Event frame so the client keeps ONE parser. **Same-day bench win
  cleared** (opposite of the napi cutover — this REMOVES work and sends fewer bytes, so the win
  survives the socket): end-to-end over a real Unix socket, REAL Rust-mirroring encoders + REAL TS
  parsers, best-of-5 @64MB/corpus — typical-shell **1.80×** (1214 vs 673 MB/s, wire −3.6%),
  control-heavy TUI **2.80×** (777 vs 277 MB/s, wire −29.4%); decoded PTY bytes byte-identical both
  ways (parity). The daughter's Claude-Code/TUI case is control-heavy — biggest win. Verified: daemon
  Rust 37/37 tests (4 frame/negotiation), TS 40 tests incl. an always-on wire-parity test, node+web
  typecheck clean; committed reproducibles `daemon-binary-stream-protocol.test.ts` (parity) +
  `daemon-stream-frame-throughput.bench.test.ts` (real Rust sender, gated). P3 the PTY
  flow-control machine as a **decisions-only Rust handle** (payload bytes stay in TS; the handle owns
  counters/gates and answers enqueue/flush/ack/heal with scalars) — safety invariants (in-flight never
  negative, caps never exceeded) as ay bundles on the orca-git precedent; liveness reformulated as
  safety/enabledness until Trust's temporal lane lands.
  **P3 stage 1 LANDED 2026-07-16** (`8a9dadc08`): new zero-dep crate `orca-flow-control` ports the
  producer flow-control decision core from `src/main/ipc/pty-producer-flow-control.ts` — the per-PTY
  hysteresis machine (pause past HIGH=256KB, resume below LOW=32KB, 5s failsafe re-assert). Pure core:
  `update(id, pending_chars, now_ms) -> Pause|Resume|None`, clock + transport injected, so it is
  deterministic and napi-ready (flow events fire at watermark crossings, never per byte — no per-chunk
  C++ hop, unlike the rejected `pty:data` cutover). 12 cargo tests prove the invariants empirically
  (exact boundaries, once-only edges, no band-flap, reassert-only-after-interval-AND-flooding, per-PTY
  independence, a full flood→drain→reflood emitting exactly [Pause, Resume, Pause]).
  **✅ P3 COMPLETE 2026-07-16** — the FIRST unit to realize the full E1 pair on real ported production
  code. **Stage 2** (`b08903433`): napi cutover REJECTED by measure-first (`updateProducerFlowControl`
  fires per-chunk across 8 delivery/ack sites → a napi hop regresses like `pty:data`); instead a
  cross-language **differential parity certificate** — TS production and the Rust spec run ONE shared
  `parity-corpus.txt` and must emit identical actions (Rust 13/13, TS 10/10). **Stage 3** (`ee9aa527f`):
  **ay machine-checked certificate** (`proofs/ay/`, `bash verify.sh` → ALL PROOFS DISCHARGED) — 4
  theorems proved ∀ (anti-flap, reassert-gated, strict low/high edges) + 2 controls (non-vacuity,
  catches off-by-one). Spec proved correct (ay) + implementations proved equivalent (parity corpus) =
  the E1 claim, demonstrated end-to-end on a shipped unit.
  **✅ 2nd E1 unit LANDED 2026-07-16** (`13f4546a8`) — the E1 recipe proven REUSABLE: keep-tail sizing
  (`orca-flow-control::keep_tail`, ported from `daemon-stream-keep-tail-drop.ts`) gets the same full
  pair. Certificate `proofs/ay/kt1..kt3` (UNSAT) prove `keep_tail ∈ [64K,512K]`, `drop_cap = 2·keep_tail
  ∈ [128K,1M]`, clamp order-preservation — the division `floor(2M/max(1,n))` abstracted as a free `x≥0`
  so the bounds hold ∀n (not sampled); `kt_c1/kt_c2` (SAT) prove both band edges are reached (tight).
  `verify.sh` now discharges **11/11**. Parity `keep-tail-parity-corpus.txt` is one shared oracle run by
  BOTH sides (Rust `matches_shared_parity_corpus` + TS `daemon-stream-keep-tail-drop.test.ts`). 19 Rust +
  11 TS green. Two shipped units now carry the full certificate+parity pair; the recipe scales to any
  scalar-decision port.
  **✅ 3rd E1 unit LANDED 2026-07-16** (`orca-provider-backoff`) — the recipe applied across a THIRD
  production subsystem (rate-limits, not PTY), and it adds a NEW invariant class the flow-control units
  didn't: a *saturating exponential*. Ports the inline refetch throttle from
  `src/main/rate-limits/service.ts` — `min(30s·2^max(0,streak-1), 15min)` — extracted to a pure TS module
  `active-failure-backoff.ts` (service now calls it) with a Rust twin. Certificate `proofs/ay/bo1..bo3`
  (UNSAT) prove `throttle∈[30s,15min]`, monotone-in-streak, and exact saturation at the ceiling — the
  exponential `2^max(0,streak-1)` abstracted as a free `p≥1` so `30000·p` is *linear* in QF_LIA; `bo_c1/
  bo_c2` (SAT) prove non-vacuity + a tight floor. `verify.sh` 5/5. A DISTINCT second obligation —
  overflow-safety of the finite-width Rust `1u64<<exp` for any `u32` streak — is separated out and
  discharged by the crate tests (called at `u32::MAX`), with bo3 supplying the soundness (past p=30 only
  `≥30` matters, so clamping a huge shift to `u64::MAX` yields the same MAX). Parity `parity-corpus.txt`
  one oracle, Rust + TS. 5 Rust + 2 TS parity green, 311 rate-limits service tests unchanged. THREE
  shipped units, two subsystems, three invariant families (hysteresis, linear clamp, saturating
  exponential) — the E1 factory is real, not a one-off.
  **✅ 4th E1 unit LANDED 2026-07-16** (`orca-crash-recovery`) — TWO more invariant families and a THIRD
  subsystem (crash-recovery), zero production-code touch (both cores were already exported classes; only
  parity tests added). Two stateful decision cores ported from `src/main/crash-reporting`:
  (a) `renderer_recovery` — a rolling-window renderer-reload rate limiter: `proofs/ay/rr1..rr3` prove
  inductive safety (≤ max attempts in any window), no-admit-at-cap, and reset-reopens liveness (no
  permanent lockout). (b) `gpu_fallback` — a one-shot GPU software-fallback latch: `proofs/ay/gf1..gf3`
  prove engages-at-most-once, window-gate no-op, and no-engage-below-threshold. Both integer-only → the
  in-window count is a free int, all QF_LIA; `verify.sh` discharges 10/10 (6 theorem + 4 control incl.
  inclusive-boundary off-by-one catches). Parity is a replayed OPERATION TRACE (like the producer
  controller), run by both the Rust core and the TS class. 8 Rust + 2 TS parity green; existing 80
  crash-reporting tests unchanged. FOUR shipped units, THREE subsystems (PTY / rate-limits /
  crash-recovery), FIVE invariant families (hysteresis · linear clamp · saturating exponential ·
  rolling-window rate limit · one-shot latch) — pure functions AND stateful machines, table AND trace
  corpora.
  **✅ E1 now GATED 2026-07-16** — the "regression-**gated**" adjective made real. New gauntlet axis
  `certificates` (`pnpm gauntlet:certificates`, in `all`; `tools/terminal-bench/gauntlet-certificates.mjs`)
  AUTO-DISCOVERS every `rust/crates/*/proofs/ay/verify.sh`, discharges each (success = exit 0, tolerant
  of both banner styles), AND runs the Rust parity corpus (`cargo test`) for every crate that ships a
  `*-parity-corpus.txt`. First green run: **5 certificate crates, 40 ay obligations discharged, 3
  decision-core parity corpora** — and it surfaced two PRE-EXISTING ay certificates (`orca-git`,
  `orca-net` Trust-parser cores) that were never enforced by anything. SKIPs (proves nothing) if ay is
  absent; REVIEWs if the parity corpora can't run for lack of a stable toolchain — never reads green on
  half the pair. The TS side of each corpus stays enforced by the vitest suite. Certificates were
  present-but-manual before; they are now a standing gate, so the E1 claim's "regression-gated" holds by
  construction and any future E1 unit is enforced with zero gate edits.
  **✅ 5th E1 unit LANDED 2026-07-16** (`orca-renderer-heap`) — a FOURTH subsystem (startup), and the
  first to cross the float boundary. Ports the already-exported pure `computeRendererHeapCeilingMb` from
  `src/main/startup/renderer-heap-headroom.ts` (zero refactor): total RAM → V8 old-space ceiling MB, or
  none. The RAM-tier decision is `clamp(floor(totalGiB·0.4)·1024, [3072,4096])` gated at 7.5 GiB, with a
  resolved env override passed in (the JS-`Number` string parsing stays TS). `proofs/ay/rh1..rh3` abstract
  the target as a free int → QF_LIA: band bound, monotone-in-RAM, and a genuine finding — rh3 proves the
  3072 FLOOR is redundant under the gate (`floor(7.5·0.4)·1024 = 3072`, so the target is always ≥ floor;
  the real clamp is the 4096 cap). `rh_c1/rh_c2` (SAT) prove both band edges reachable. verify.sh 5/5.
  The float layer (JS `Number` ≡ Rust `f64`, bit-identical `/2^30 · *0.4 · floor · as u32`) is pinned
  EMPIRICALLY by the parity corpus run against the real TS function (parser included) — not asserted.
  5 Rust + 1 TS (13 w/ existing) green. **The certificates gate auto-picked-it-up with ZERO edits: 6
  certificate crates / 45 obligations / 4 parity corpora.** FIVE units, FOUR subsystems (PTY /
  rate-limits / crash-recovery / startup), FIVE invariant families, gated.
  **✅ 6th E1 unit LANDED 2026-07-16** (`orca-stream-split`) — a FIFTH subsystem (daemon TRANSPORT, the
  moonshot's own spine) and a qualitatively different core: an ALGORITHM primitive, not a scalar/threshold.
  Ports the surrogate-safe split-index functions `clampToSafeSplitIndex` + `nextSafeSplitIndex` from
  `src/main/daemon/daemon-stream-data-split.ts` (the primitive the keep-tail dropper already depends on —
  thematic closure). These choose stream-chunk boundaries that never cut a UTF-16 surrogate pair. Pure
  over UTF-16 code units → bit-exact parity (the NDJSON-byte-budget binary search that drives them stays
  TS). `proofs/ay/cs1..cs2, ns1..ns2` prove: clamp never splits the target pair, clamp stays in
  [start,end], next always makes forward progress (no stall), a pair at `start` is skipped whole;
  `cs_c1/ns_c1` (SAT) both fixes fire. **New ay technique: code units modelled as free ints in [0,65535]
  with the surrogate ranges as LINEAR bounds (high 55296..56319, low 56320..57343)** → pure QF_LIA, no
  strings/bit-vectors. verify.sh 6/6. Corpus is real surrogate pairs (😀 = d83d de00). One-word prod
  change (`export nextSafeSplitIndex`). Gate auto-enforced: **7 certificate crates / 51 obligations / 5
  parity corpora.** SIX units, FIVE subsystems, SIX invariant families — scalar decisions AND a string
  algorithm, pure functions AND stateful machines, all gated.
  **✅ 7th E1 unit LANDED 2026-07-16** (`orca-session-gc`) — the meatiest unit: a real production REFACTOR
  (not just a lift), the daemon **history-retention** subsystem, and a 7th invariant family
  (budget-respecting multi-class eviction with liveness exemptions). Extracted a pure
  `planSessionHistoryGc` from `runDaemonSessionHistoryGc` (`src/main/daemon/history-retention.ts`) — the fs
  scan + rmSyncs stay in the executor, the age-expiry + oldest-first size eviction become a pure planner
  the executor applies; all **12 existing history-retention integration tests still pass** (behavior
  preserved). `proofs/ay/ex1..ex3` prove the privacy-critical retention guarantees (a LIVE dir is never
  expired; the TOCTOU floor; unknown-liveness not-ended → ∞ retention), `ev1..ev_step` the eviction
  guarantees (remaining ≥ non-evictable floor so live/recoverable bytes are never traded for disk; reaches
  budget when enough evictable; each step monotone); `ex_c1/ev_c1` (SAT) non-vacuity. Flags as Bools,
  byte totals as free ints → QF_LIA; verify.sh 8/8. Parity corpus is 12 cases (every expiry branch +
  oldest-first, spare-live, liveness-unknown-evictability, tie-break, combined expiry+eviction, empty).
  3 Rust + 1 TS (13 w/ integration) green, clippy+oxlint+typecheck clean. Gate auto-enforced: **8
  certificate crates / 59 obligations / 6 parity corpora.** SEVEN units, SIX subsystems (PTY /
  rate-limits / crash-recovery / startup / daemon-transport / daemon-retention), SEVEN invariant families
  — the first with a behavior-preserving production refactor as its lift.
- **T1 Equality escalation** [XL, the deepest lever]: the scalar equality-`ensures` lane **already
  landed 2026-07-04** [recorded, `~/trust/reports/`]; the open remainder is interprocedural
  `assert!(candidate(x) == spec(x))` — wire the existing whole_program.rs callee-summary lane into
  production. Prize: flagship kernels upgrade from tested-parity to **SmtBacked ∀-equivalence** —
  "proven-equivalent kernels."
- **E1 The claim, worded exactly** (from the precedent research; every neighbor claims a strict subset):

  > *The first published system that migrates production TypeScript to Rust with machine-checked safety
  > certificates on the emitted code plus regression-gated behavioral parity corpora, deployed in a
  > shipping product.*

  Not "first verified transpilation" (VERT owns it), not "first formally verified migration" (Heimdall,
  eBPF micro-programs), not "proven equivalent" (parity corpora are testing evidence — until T1 lands
  for select kernels). Venues: ICSE-SEIP / FSE-Industry (experience report with operational data),
  OOPSLA / PLDI + artifact evaluation (toolchain contribution), CAV industry (certificate
  infrastructure). Pre-scripted rebuttals: Cedar (proofs on a Lean model, not production code; greenfield
  not migration), Heimdall (tiny eBPF C, no target-side certificates, unshipped), Corsa/typescript-go
  (largest TS-to-native migration, **zero** formal guarantees), "Android shows you don't need proofs"
  (answer with what the certificates caught that the 137-test corpus alone did not).

---

## 10. Campaign 6 — Native Photons + libaterm (the endgame)

1. **Daemon subscriber role** (fork protocol 1018→1019 — the fork namespace is deliberately away from
   public v18–22 [review-corrected]): read-only fan-out alongside owner attach — snapshot hydration,
   resize denied (the placeholder-grid SIGWINCH-bounce lesson is codified: followers pin to the
   owner's grid). This is the hidden prerequisite for every two-frontend story.
2. **Detach-to-native wedge → aterm-gui workspace mode**: two frontends, one daemon. Honesty note
   (review-corrected): the `native/orca-macos` SwiftUI spike is a 30fps-polling toy (nested Text
   cells, direct shell spawn) — it proves appetite, not architecture; the wedge starts from aterm-gui,
   not from it. Native cold start plausibly <100ms vs Electron's 552ms [recorded baseline];
   keypress→photon at the WindowServer floor — the Chromium compositor tax (~1 frame+) exits the
   equation. Local-only at first; native SSH parity is its own phase gated on the orca-ssh transport
   port.
3. **libaterm** [M]: the embeddable **proof-carrying** terminal engine — the libghostty strategy with a
   moat Ghostty structurally cannot copy quickly (certificates travel with the library). The wasm
   competitor is already real: ghostty-web (~400KB, xterm.js-compatible, "not yet optimized"
   [external]). Beat it on throughput, latency, and bundle size, and ship the **run-it-yourself browser
   race**: a page where any reader races aterm-wasm vs xterm.js in their own tab, parse-only and
   on-glass legs labeled separately.

### The coordinator: what stays Orca's, what becomes ours

Directive (2026-07-15): the end product Andrew wants is a **super-coordinator of aterms — each aterm a
window or an agent**. Identity policy:

- **The sovereign artifact is not the Electron app.** It is the stack underneath it: the Rust daemon
  (sessions, PTYs, soon git/fs), the wire protocol (v18→19 with the subscriber role), the engine
  (aterm/libaterm), the proofs, and orc-electron. "Something of our own" **already began** at Move 1 —
  the daemon extraction; every campaign above deepens it.
- **The orc fork preserves ~all Orca functionality indefinitely** (workspaces, worktrees, agent
  orchestration, review/Linear/GitHub surfaces, SSH/WSL, mobile). The TS product surface + upstream-first
  cadence exist precisely so upstream merges stay cheap. Orca-the-client remains the daily driver and
  the distribution vehicle for the engine/runtime/daemon work — and the proof that existing Electron
  apps can be made amazing.
- **The coordinator is a NEW, thin client of the same daemon — not a port of Orca's surface.** Because
  sessions/agents/terminals live daemon-side, the coordinator starts as: attach → session grid → agent
  status → orchestration controls, with each pane an aterm view (Electron window, native aterm-gui
  window, or both — two frontends, one daemon). Orca-specific product UI is *not* ported; it stays in
  the Orca client. The coordinator grows by value, never by parity checklist (the Move-3 rule).
- **Divergence policy:** keep merging upstream while Orca-the-client is the primary surface; the
  question "when do we stop tracking upstream" answers itself the day the coordinator becomes the daily
  driver — and until then the fork keeps compounding on upstream's feature work for free.
- **Sequence:** subscriber protocol rev [M] → detach-to-native wedge [M] → coordinator v0 (session
  grid + agent status over the daemon) [L] → coordinator as primary surface [XL, its own roadmap].

---

## 11. The claims board (what gets announced, in claim-safe wording)

1. **An Electron app that out-ingests Ghostty.** End-to-end ≥260 MB/s local [target] where the kernel
   floor allows — measured macOS single-PTY cooked floor is ~119 MB/s [recorded], so the record run
   uses raw-mode/Linux/multi-stream, or the claim is stated as "saturates the kernel PTY floor";
   camera on the pipe, floor published alongside.
2. **The lowest-latency browser-tech app ever measured.** ≤10ms typometer class [target] vs Hyper 39.8 /
   VS Code 31.2 [external]; mid-pack among *native* terminals, stated per-methodology, per-refresh-rate.
3. **"The parser IS the spec."** 3,584 machine-checked transitions + a delta ledger, gating every build.
4. **Memory by theorem.** The 24h soak flat line annotated with certificate IDs; *RAM ≤ budget + K* proven.
5. **SSH that echoes in ~8ms** at any RTT, with a proven-safe predictor — mosh parity plus the theorem
   mosh never had.
6. **The autoformalization factory.** The exactly-worded first (Campaign 5 E1) — the claim survives
   VERT/Heimdall/Cedar/Corsa comparison by construction.
7. **`bash verify.sh`.** One command, our toolchain: ships `ay`/`ty` (prebuilt or seed-bootstrap) and
   re-checks every bundle; headline bundles reconstruct to kernel-Certified; bundles are standard
   SMT-LIB2/CHC (z3-checkable — verified live) so independent confirmation is there for anyone who
   wants it. Every headline number links a ledger row and, where claimed, a theorem.

---

## 12. Sequencing — dependency waves, not calendar time

The work is agent-executed; ordering is by prerequisite and gate, never by weeks. Effort tags (S–XL)
size the work, not its date.

**Wave 1 — unblocked now (S):** SAB flag + flag sweep (rung 1) ✅ *landed 2026-07-16 (SharedArrayBuffer
composed on all paths incl. Linux-E2E; bench-only vsync flags behind `ORCA_BENCH_RUNTIME_FLAGS=1`)* ·
package `ay`+`ty` into verify.sh (T-A) · SIMD128 build flags ✅ *landed 2026-07-16 (+simd128 target-scoped
+ wasm-opt --enable-simd; blobs rebuilt, 18,704/27,157 vector instructions cpu/gpu vs 0 before,
behavior-neutral on the 26-case corpus; v128 scanners remain upstream work)* · kernel-PTY-floor ✅
*landed 2026-07-16 (macOS ~119 MB/s recorded)* + typometer instruments · census in the gauntlet ✅
*landed 2026-07-16 (`pnpm gauntlet:census`, regret-class ratchet)*.

**Wave 2 — needs Wave 1's instruments (M):** ✅ **coordinator v0** (attach → session grid → attention
queue → read-only aterm tiles in the focused view, wearing Orca's design system — the Goal-2 product;
milestones A+B landed) · ✅ **P2 binary daemon frames** (v1020 opt-in binary stream plane; napi-string
NDJSON cutover was measured 30% slower and rejected, Buffer-payload frames landed 1.8–2.8× end-to-end)
· utilityProcess pump spike · dirty-band CPU present · `orca://` migration + codeCache · parser
spec-table + delta ledger gate · F1 provenance gate · F2 trace corpora · orc-electron fork infra
(repo, sccache, no-op rebuild ×3 OS) · ✅ daemon subscriber protocol rev (1018→1019→1020).

**Wave 3 — needs Wave 2's protocol/runtime footholds (L):** byte ACK re-base · verified transport
binding (a) (renderer plane) · predict.rs extraction + echo theorem + remote gate ·
crossOriginIsolated + wasm threads (via the origin-isolation patch, retiring the webview COEP risk) ·
first orc-electron patch set (isolation + macOS low-latency canvas) · F3 swc/oxc front-end ·
F4 factory loop · P1/P3 ports · detach-to-native wedge · scoreboard v1 (browser race + latency table +
DECRQCRA prerequisite for the esctest leg).

**Wave 4 — gated on capability rungs and Wave 3 evidence (XL):** byte-path Trust campaign (milestone:
aterm-parser through `targo trust check`) · Trust ladder T-B..T-G · verified-transport bindings (b)(c) ·
coordinator → primary surface + true one-parse protocol · libaterm · component stripping +
PGO/pointer-compression variants · custom Viz surface once bench evidence localizes the compositor tax.

---

## 14. External review triage (2026-07-15 — codex gpt-5.6-sol/ultra primary, gpt-5.5/xhigh secondary)

Both reviews ran with repo access against these docs. Verdicts said "don't green-light as written";
the program's answer is: **facts adopted, deflation rejected** — ambition stands, sequencing and claims
got honest. Adopted (and applied above):

- "One parse" was false as scoped → Campaign 1 item 5 retitled **3→2**; true one-parse is the Wave-4
  protocol endgame. `+simd128` alone doesn't vectorize (wasm scalar fallback at `simd.rs:462`) → the
  v128 scanner family is the deliverable. Predictor claim narrowed to display-isolation (Always-mode
  secret-paint caveat). Verified-transport stalled-visible-client policy pinned to producer
  backpressure. Fork protocol numbering corrected (1018, not v18). Windows daemon status precised:
  named-pipe source exists but is uncompiled/unpackaged (`build-rust-daemon.mjs:9` skips it) with
  security gaps (winpipe default security attributes; token file lacks owner-only DACL) — now explicit
  Windows-lane work items. `native/orca-macos` demoted from "de-risks endgame" to appetite spike.
  The 411ms longtask was a *renderer* tab-create measurement, not main-process-stall evidence — the
  utilityProcess pump keeps its rationale (protocol hygiene + isolation) minus that citation.
- **New gates adopted**: per-hop throughput budget in Campaign 0 (PTY floor → daemon → socket → port →
  wasm, each measured before any composite ≥260 MB/s claim); a wasm **bundle-size gate** on the
  scoreboard (shipped blobs are 3.4/5.9MB vs ghostty-web's ~400KB); a **daemon threat model** work item
  (per-client authz, revocation, redaction, protocol fuzzing, isolation-patch implications); Goal-1
  **acceptance gates** (e.g. ≥2 upstream-merged PRs, an upstream sponsor for the next slice) so
  "upstream-adoptable" is measured, not asserted; per-campaign **kill criteria**.
- **Priority inversion fixed**: coordinator v0 moved to Wave 2 (it needs only owner-attach); its
  product gates live in the blueprint (attention queue, one-click resume, approvals, recovery,
  time-to-first-success) — a grid of terminals is infrastructure, not the product.

Rejected, with reasons: "park everything except a coordinator mode inside the existing client" —
rejects the sovereign-stack logic and the owner's explicit ambition directive; the campaigns are
agent-executed waves with gates, not a solo engineer's quarter. "Team-of-one capacity" — execution is
by agent fleets gated by the gauntlet (this program's standing operating model); kill criteria adopted
instead. "Upstream won't adopt the factory/proofs" — they were never for upstream; they are Goal 1 as
a business (client #1: Orca) and the publishable moat. Review artifacts: session scratchpad
`moonshot/codex-review-56-ultra.md` and `codex-review-final.md`.

---

## 13. Provenance

Synthesized 2026-07-15 from three workflow runs (IDs `wf_3efdd2b5`, `wf_2b22a5d3`, `wf_ad1b2019`;
journals under the session transcript dir). Dive/verdict JSON extracts preserved in the session
scratchpad (`moonshot/*.json`). Standing constraints: product surface stays TypeScript
(AGENTS.md + memory directive 2026-07-15); SSH/WSL and macOS/Linux/Windows keep working; upstream-first
cadence for aterm changes; no GitHub CI — the gauntlet is the gate.
