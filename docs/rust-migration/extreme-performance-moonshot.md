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
   ∀-safety proofs, the ts2rust two-witness harness (141/203 TRUSTED on real orc code [recorded]), the
   `ay` SMT/CHC solver, a kernel-Certified spine (86/86 [recorded]).
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
| App end-to-end ingest | **2–15 MB/s** [recorded] — the pipe, not the engine |
| Current public record (Ghostty nightly) | **~260 MB/s** ingest [external] |

Per chunk today [recorded, data-path dive]: **4 process/thread hops, 3 full terminal parses** (daemon
headless emulator, main headless emulator, worker wasm), **~6 UTF-8⇄UTF-16 transcodes, ~16 buffer
copies, 4 independent batching queues, 4 distinct backpressure systems**, bytes riding **NDJSON JSON
strings**. A binary length-prefixed frame codec exists in-tree and is unused
(`src/main/daemon/binary-frame.ts`). The Rust daemon already owns PTYs by default on macOS/Linux
(`src/main/daemon/daemon-init.ts:282-291`).

Everything in Campaign 1 exists to close the gap between 2–15 and ~300. The engine is already there.

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

**The audit's key discovery:** two tracks exist and have never been fused. Track 1 (shipped): LLM-agent
hand-ports with verbatim test translation, parity corpora (1,149 cases), ay safety bundles, and a proven
promotion recipe (parity → napi/wasm via orca-dispatch → shadow cutover → delete the TS twin — orca-git
landed this way: 137 tests, 10 SMT obligations, TS deleted 2026-07-06). Track 2 (unpromoted): the
ts2rust two-witness autoformalizer — W1 `trustc` ∀-safety, W2 Node-TS differential fuzzing —
**141/203 TRUSTED** on real orc code [recorded], outputs sitting in `~/trust/tools/ts2rust/orca`,
never shipped. The factory = fuse them.

- **F1 Provenance gate** [M]: pin every Rust port to its TS source hash; the gauntlet fails with a
  structured re-port task on drift. (Last upstream merge caught 5 shadow-port drifts *reactively*.)
- **F2 Trace-derived corpora** [M]: record (input, output) pairs at the orca_dispatch seam and from
  vitest runs; publish Cedar-style corpus metrics.
- **F3 Real TS front-end** [L]: vendor swc/oxc — inferred argspecs, auto-extracted oracles, generated
  Rust skeletons; agents only fill `todo!()` bodies. Target: an order-of-magnitude drop in
  agent-minutes-per-TRUSTED-pair.
- **F4 Close the loop** [L]: unattended classify→port→verify→promote for in-fragment kernels **whose
  signature matches the live export** (the verifier's key restriction — TRUSTED kernels with narrowed
  types can't ship as-is); promotion re-runs autoformalize against the real module source; ships
  through the existing one-export orca-dispatch seam. Inventory honesty: 141/203 TRUSTED today, not 247.
- **Port targets by measured heat:** P1 the onPtyData chunk-ingest core as one Rust scan pass
  (**UTF-16 code-unit seam mandatory** — napi string conversion replaces lone surrogates and PTY chunks
  split astral pairs; re-baseline heat on current main first). P2 — **re-scoped by measurement
  2026-07-16**: the napi-string `RustNdjsonParser` cutover was implemented, proven wire-identical
  (parity green), benched, and REJECTED — ~30% slower end-to-end than the TS parser (458 vs 657 MB/s
  full pipeline; split-only 810 MB/s vs 4.5 GB/s) because per-line UTF-16⇄UTF-8 FFI copies dominate
  while V8 substrings are copy-free [recorded]. The old parity-test comment predicted exactly this;
  the bench gate held. P2 is therefore **binary frames with Buffer payloads only** (near-zero-copy
  napi externals) — string-shaped FFI on hot paths is a proven dead end; the manifest rule: no Rust
  cutover on a hot path without a same-day bench win. P3 the PTY
  flow-control machine as a **decisions-only Rust handle** (payload bytes stay in TS; the handle owns
  counters/gates and answers enqueue/flush/ack/heal with scalars) — safety invariants (in-flight never
  negative, caps never exceeded) as ay bundles on the orca-git precedent; liveness reformulated as
  safety/enabledness until Trust's temporal lane lands.
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

**Wave 2 — needs Wave 1's instruments (M):** **coordinator v0** (attach → session grid → attention
queue, wearing Orca's design system — the Goal-2 product starts here, before the records; needs only
owner-attach, not even the subscriber rev) · binary daemon frames · utilityProcess pump spike ·
dirty-band CPU present · `orca://` migration + codeCache · parser spec-table + delta ledger gate ·
F1 provenance gate · F2 trace corpora · P2 binary frames (napi-string NDJSON cutover measured 30%
slower and rejected — Buffer payloads only) · orc-electron fork infra (repo,
sccache, no-op rebuild ×3 OS) · daemon subscriber protocol rev (1018→1019).

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
