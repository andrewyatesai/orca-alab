# Trust Performance Migration Plan

> **Status (2026-06-28):** Phase 0 (locale lazy-load) shipped. `orca-git` is the first
> fully-landed subsumption: verified pure-Rust core + `ay` proofs → napi exposure →
> dual-run parity (43/43) → **live cutover** of `getStatus` behind the TS fallback. The
> recipe below is now proven end-to-end; everything in §6 follows it. See
> [§7 Aterm responsiveness roadmap](#7-aterm-responsiveness-roadmap) and
> [§8 Trust subsumption order](#8-trust-subsumption-order-what-to-pull-in-next) for the
> forward plan.

> Goal: migrate orca's biggest **user-experience performance bottlenecks** into
> **Trust-verified Rust** (aterm's "Trusted Rust" toolchain — `trustc` + `ay` SMT/CHC
> + `trust-mc` BMC, the `proofs/ay/` discipline). This is a *performance* plan, not the
> whole-app port roadmap in [`migration-plan.md`](./migration-plan.md); it ranks only the
> hot paths where moving to Rust measurably helps the user, and says which of those can
> additionally carry a formal proof.
>
> Evidence basis: a three-stream profiling pass (terminal data path / startup+CPU / Trust
> mechanism) on `2026-06-28`, against aterm submodule `v0.5.3` (`707c65b`). All file:line
> anchors below were read directly; re-verify before editing — code moves.

---

## 0. The proof boundary (what Trust can and cannot cover)

From [`rust/PROOF_CARRYING_PERFORMANCE.md`](../../rust/PROOF_CARRYING_PERFORMANCE.md):
**the proof line is the FFI surface, and Trust only covers logic expressible over an
abstract integer/byte domain** — index-in-bounds, no-overflow, totality (never panics),
range/gamut, inductive byte-budget invariants. Anything touching the DOM, `devicePixelRatio`,
CSS measurement, or device pixels stays in TypeScript and is covered by TS unit + Playwright
gates, **not** a Rust theorem.

Consequence for this plan: a bottleneck is a **strong Trust target** only when its hot core
is a byte/integer computation (a parser, a codec, an index, a ring buffer, a score). A
bottleneck that is mostly IPC hops or DOM coupling should be fixed *in place* (workers,
batching, code-split) — migrating it to Rust buys nothing and a proof would be vacuous.

Two honesty caveats carried from the research:
- **orca-side crates carry no Trust proofs today.** The entire Trust apparatus lives inside
  the `rust/aterm` submodule workspace. orca-side code (`rust/crates/*`, `native/orca-node`)
  currently ships *unverified* (`orca-terminal` even `forbid`s `unsafe`). Getting Trust on a
  new orca-side crate means **porting the proof infra** (a `proofs/ay/` dir + `verify.sh` +
  the `cfg(trust_verify)` declaration), or placing the crate inside the aterm submodule.
- **The Trust toolchain is environment-gated.** Per `rust/aterm/AGENTS.md`, on a given
  machine only some checkers (`ty`) are guaranteed runnable; `ay`/`trust-mc` need the
  `~/trust` toolchain. Proofs are authored in-idiom and discharged where the toolchain is
  present (the owner's box + the `tools/verify.sh --full` gate), skipped-not-failed elsewhere.

---

## 1. Bottlenecks ranked by (UX impact × Trust fit)

| # | Bottleneck | UX impact | Trust fit | Seam | Crate placement |
|---|------------|-----------|-----------|------|-----------------|
| **1** | Git status porcelain parse + C-quoted decode + line counting | High (runs every few-hundred ms while editing) | **Strong** (byte tokenizer, bounds/overflow/totality) | napi | `orca-git` (orca-side) |
| **2** | Terminal output byte pipeline: OSC/title/bell scan + main-process batcher + double transcode | High (throughput + frees main thread → keystroke latency) | **Strong** (byte scan + budget-bounded ring) | wasm + napi | aterm submodule (Option A) |
| **3** | File-tree projection + fuzzy ranking (per-keystroke, UI thread) | Med-High (typing jank in Explorer / Quick Open) | Moderate (score/index provable; collation is the catch) | wasm | `orca-text` (orca-side) |
| **4** | CPU-fallback framebuffer double-copy | Med (only the CPU/SSH/VM render path) | Weak (it's a redundant JS copy, not new logic) | wasm | aterm (already owns it) |
| **5** | Web/mobile E2EE crypto + base64 | Med (web client only; all I/O flows through it) | Strong-but-deep (constant-time, no-overflow) | wasm | `orca-crypto` (orca-side) |

**Explicitly NOT Rust migrations** (fix in place — migrating them is wasted effort, see §4):
locale JSON eager-parse, the 9.4 MB eager renderer chunk, the ~12 serial boot IPC round-trips,
the ripgrep `--json` parse, the Electron fs-watcher fan-out, the keystroke input path itself.

---

## 2. The migrations, in detail

### #1 — `orca-git`: status/diff byte layer (napi) — *flagship*

The highest sustained Rust ROI and the cleanest Trust target, so it goes first and
establishes the orca-side Trust pipeline.

- **What's hot today (TS):**
  - `src/main/git/status-porcelain-parser.ts:47,130-184` — porcelain v2 parse (local).
  - `src/relay/git-status-output-parser.ts:43` — the SSH/relay variant, with **no early-stop**:
    it materializes the entire array (200k+ untracked files) before truncating.
  - `src/shared/git-uncommitted-line-stats.ts:125-136` — a hand-rolled newline byte-loop that
    reads each file up to 2 MB.
  - `src/renderer/src/components/editor/diff-line-stats.ts:5-39` — per-section line-multiset Map.
  - Driven by the worktree fs-watcher (debounced 150/500 ms) **plus** every stage / discard /
    commit / branch-switch — i.e. constantly during active editing. Cap is
    `DEFAULT_GIT_STATUS_LIMIT = 10_000` (`src/shared/git-status-limit.ts:6`).
- **Rust target:** one native git-status module — porcelain v2 tokenizer with the cap applied
  *during* the scan (fixes the relay's full-materialize bug for free), `decodeGitCQuotedPath`,
  newline counting (`memchr`/SIMD), and numstat/name-status parse. Unify the duplicated
  `src/main/git` + `src/relay` logic into the single crate before porting.
- **Seam:** napi (main + relay processes; no DOM, large/streaming buffers). Extend
  `native/orca-node` with new `#[napi]` methods, or add a sibling addon built like
  `config/scripts/build-terminal-addon.mjs`.
- **Trust proof (Tier-0 SMT, mirrors aterm's `a2_codec`):**
  - tokenizer field offsets stay in-bounds across chunk boundaries;
  - the byte/line accumulators never overflow `u32`/`usize`;
  - `decodeGitCQuotedPath` is **total** (every input produces output, never panics);
  - the cap invariant: emitted entries `≤ limit` for all inputs (the property the relay bug violated).
  Author as `rust/crates/orca-git/proofs/ay/git_porcelain/` (`.smt2` + `verify.sh` + `README.md`),
  cloning the `a1_row_index` bundle structure.
- **Risk/blocker:** must preserve the streaming early-stop + chunk-boundary carry contract;
  SSH/relay needs a per-relay-arch native build (the real blocker — keep the TS path as the
  fallback when no addon for that arch); WSL path translation must be preserved.

### #2 — Terminal output byte pipeline (Option A: inside aterm)

Three coupled wins on the hottest UX path; all are engine-general, so they go **inside the
aterm submodule** and ride aterm's existing `tools/verify.sh` proof gate (free Trust infra).

- **2a. Fold OSC 9999 / title / bell scanning into the engine.**
  - Hot today: `src/shared/agent-status-osc.ts:38-85` (OSC 9999, **every chunk**),
    `extractAllOscTitles` + `bellDetector.chunkContainsBell` via
    `src/renderer/src/lib/pane-manager/aterm/pty-transport.ts:209-283`. That's 1-3 full-chunk
    string scans + allocations per chunk on the renderer thread *before* bytes reach the engine.
  - The engine already parses OSC and exposes a drained event channel
    (`controller.takeOscEvents`, `aterm-terminal-facade.ts:51-74`). Extend it to emit OSC 9999 /
    title / bell as structured events; delete the TS re-scan.
  - Trust: OSC parser totality + the stateful-across-split-chunks strip is correct (it's an
    orca control protocol that must be stripped before display) — a parser-totality obligation
    of the same shape aterm already proves for its codec.
- **2b. Move the main-process output batcher to a Rust byte-ring.**
  - Hot today: `src/main/ipc/pty.ts:1339-1354,1381-1391` — `existing.data + data` concat per
    chunk and `slice(0,CHUNK)`/`slice(CHUNK)` per 8 ms flush, on Electron's **main** thread,
    which also services every `pty:write` and all IPC. Main-thread block here adds latency to
    *all* panes and to keystroke delivery.
  - Rust target: a per-PTY byte ring buffer that coalesces in Rust and hands the renderer byte
    slices, in the napi PTY provider.
  - Trust (mirrors aterm's `budget_chc`): an inductive invariant that the buffered byte count
    stays bounded across pushes/flushes (no unbounded growth → OOM-impossibility), and the
    backpressure/ACK accounting (`rendererInFlightCharsByPty`) is preserved.
- **2c. Eliminate the double UTF-8 transcode.**
  - Hot today: node-pty decodes bytes→string in main, the string is structure-cloned across
    IPC, then `aterm-process-pump.ts:21-28` does `encoder.encode(data)` to get bytes *back* for
    `term.process(bytes)`. Two transcodes + an IPC string clone per chunk → steady renderer GC
    that competes with paint and input.
  - Rust target: byte-mode PTY read in the napi provider → deliver raw bytes to the renderer as
    a transferable `ArrayBuffer` → straight into `process(bytes)`. **Sequenced after 2a**: the
    string consumers (OSC scan) must move to bytes/engine first, or one decode stays.
- **Seam:** wasm (2a, engine OSC) + napi (2b/2c, main-process byte transport).
- **Net effect:** removes 1-3 string scans/allocs per chunk on the renderer thread, the
  main-thread concat/slice churn, and one full transcode per chunk — the throughput + jank win,
  and indirectly a keystroke-latency win (less main-thread contention).

### #3 — `orca-text`: file-list index + fuzzy ranking (wasm)

- **What's hot today (TS, renderer UI thread, per keystroke, undebounced):**
  - `src/renderer/src/components/right-sidebar/file-explorer-name-filter-projection.ts:147-239`
    (rebuild + recursive `localeCompare` sort);
  - `…/useFileExplorerVisibleRowProjection.ts:101-117,234-256` (double DFS per render);
  - `src/renderer/src/components/quick-open-search.ts:38-103` (`rankQuickOpenFiles`,
    O(N·pathLen) over the full `listRuntimeFiles` set per keystroke).
  - In a monorepo (tens of thousands of entries) this is direct typing latency.
- **Rust target:** build the wasm-side index **once when the panel opens** (marshal the big
  `string[]` across the boundary one time, *not* per keystroke — or the win evaporates); rank
  per keystroke returning ordered indices / `{path,score}[]`.
- **Seam:** wasm (renderer). Add the crate to `config/scripts/build-aterm-wasm.mjs`'s `CRATES`
  map and load it via a `load-orca-text.ts` mirroring `load-aterm.ts`.
- **Trust:** score arithmetic stays in range and the returned indices are in-bounds of the
  index (Tier-0 SMT, `a1_row_index` shape).
- **Risk/blocker:** `localeCompare` collation needs an ICU-equivalent in Rust (`icu_collator`)
  or the sort order visibly changes — decide and document. Output is React-coupled (`TreeNode`),
  so only the ordering/index computation crosses the boundary, not the tree objects.
  Because this is orca-specific, it's an **Option B** crate → it needs the Trust infra ported
  (do this *after* #1 proves the orca-side pipeline).

### #4 — CPU-fallback framebuffer single-copy (quick win, low Trust content)

- `aterm-frame-painter.ts:63-78`: `ctx.putImageData(new ImageData(new Uint8ClampedArray(term.rgba()), …))`
  copies the device-pixel buffer out of wasm (`rgba()` already `.slice()`s) **and then copies it
  a second time**. A fullscreen Retina pane ≈ 32 MB of memcpy/frame on the CPU path (GPU path is
  zero-readback and already default on capable hardware — `aterm-gpu-auto-policy.ts:78`). The CPU
  path is exactly the SSH/RDP/VM/headless case AGENTS.md cares about.
- This is **not really a Trust migration** — it's deleting one redundant JS copy and having the
  engine write into a reused buffer. List it as a fast perf fix, not a verified-Rust task.

### #5 — `orca-crypto`: web/mobile E2EE (wasm) — *later*

- `src/renderer/src/web/web-e2ee.ts:43,59,63-80` — tweetnacl `box`/`box.open` (pure JS,
  ~50-100 MB/s) + char-by-char base64, on the web client's main thread; **all** terminal output /
  diffs / file reads / search results flow through it when driving the desktop from phone/web.
- Rust target: `crypto_box` (X25519) + base64 → wasm, wire-compatible (nonce‖ciphertext).
  Strong Trust target (no-overflow, constant-time) but crypto verification is deep and it's
  web-client-only → schedule after #1-#3.

---

## 3. Phasing

- **Phase 0 — non-Rust unblockers (parallel, no Trust):** the §4 quick wins. Do these first/with
  Phase 1 so the Rust effort targets the genuinely CPU-bound work, not startup-IPC or parse-cost
  that code-splitting fixes. Includes #4 (framebuffer single-copy).
- **Phase 1 — `orca-git` status/diff (napi) + the orca-side Trust pipeline.** Flagship. Proves we
  can author `proofs/ay/` + `verify.sh` for an orca-side crate and gate it. Unblocks every later
  Option-B crate.
- **Phase 2 — terminal byte pipeline (aterm submodule):** 2a OSC-into-engine → 2b Rust batcher →
  2c byte transport (in that dependency order). Rides aterm's existing proof gate.
- **Phase 3 — `orca-text` fuzzy/index (wasm):** once Phase 1 has established orca-side Trust.
- **Phase 4 — `orca-crypto` E2EE (wasm):** web/mobile path.

Each phase ships behind the existing TS fallback (napi: `rust-terminal-addon.ts` already falls
back to the TS emulator when the `.node` is absent; wasm: keep the TS path until the crate is
proven on the target arch). Dual-run, never a hard cutover.

---

## 4. NOT-Rust wins to land alongside (so no Rust effort is wasted)

These dominate *startup* and some jank but are code-split / worker / batching fixes, **not**
Rust migrations. They are the highest-ROI perf work overall and must not be confused with the
Trust migration:

- **Locale JSON lazy-load** — ~3.14 MB of locale JSON is eagerly embedded **and parsed twice**
  per launch (`src/renderer/src/i18n/i18n.ts:4-8` + `src/main/i18n/main-i18n.ts:4-8`), ~35% of
  the 9.4 MB index chunk. Dynamic-`import()` only the resolved locale; inline English as the sync
  fallback. **Single highest-ROI startup change.**
- **Code-split the 9.4 MB eager renderer entry** (`main.tsx`→`App.tsx`, eager Sidebar/RightSidebar).
- **Batch the ~12 serial boot IPC round-trips** with `Promise.all` (`App.tsx:845-979`); only
  `fetchSettings` must lead. Latency-bound, not CPU.
- **ripgrep `--json` parse off the main thread** (`src/shared/text-search.ts:278`) → a Node
  `worker_thread`. `JSON.parse` is already native C++; the win is unblocking the event loop, not Rust.
- **Electron fs-watcher fan-out** (`src/main/ipc/filesystem-watcher.ts:142-266`, up to 5000 paths)
  → a worker (the runtime/serve watcher already uses one).

Bundle-composition facts (so we don't chase phantoms): `ts.worker` 12.8 MB = Monaco's TS language
service (off-thread, loads only when an editor opens); `scroll-cache` 8.7 MB = Monaco core + TipTap +
mermaid (Vite just named the lazy chunk after the tiny `lib/scroll-cache.ts` LRU) — **not**
terminal-related, lazy-loaded. Already native, do not migrate: node-pty, @parcel/watcher (in a
worker), ssh2, sherpa-onnx STT, electron safeStorage, monaco + `onig.wasm`, system `rg` (not bundled),
and the aterm engine itself (wasm renderer + `orca_node` napi).

---

## 5. Migration recipe (how to take one item from TS → Trust-verified Rust)

1. **Pick the seam.** Renderer-thread pixel/grid/text/score work → **wasm** (no libc; gate
   disk/zstd features off as `aterm-wasm` does; single shared instance, competes with UI thread).
   Main/daemon session/parse/serialize/crypto/git work → **napi** (Node ABI-stable; favor
   coarse-grained batched calls over chatty per-byte ones — the boundary crossing is the cost).
2. **Place the crate.** *Option A* — inside `rust/aterm/crates/<name>`: inherits `cfg(trust_verify)`
   (`rust/aterm/Cargo.toml:107`), the `[patch.crates-io]` aterm-trust forks, the `proofs/ay/`
   infra, and `tools/verify.sh` in the merge gate — but must be engine-general and flows through
   `bump-aterm.mjs`. *Option B* — orca-side `rust/crates/orca-<name>` (member in `rust/Cargo.toml:14`):
   normal PR flow, but you must **port** a `proofs/ay/` dir + `verify.sh` + the `check-cfg` entry,
   and make `~/trust` reachable. Default: Option A for genuine engine/codec logic wrapped by a thin
   orca-side adapter (the existing `orca-terminal → aterm-core` anti-corruption pattern); Option B
   only for orca-specific logic.
3. **Attach the proof** (cheapest tier that covers the property; follow
   `assert_proves_and_catches` — one `unsat` theorem + one `sat` non-vacuity + one `sat` false-bound
   catch):
   - *Tier-0 SMT (`ay`):* `proofs/ay/<name>/` with hand-encoded `.smt2` over `QF_BV` modeling the
     arithmetic (index bounds, no-overflow, range), a `verify.sh` cloned from a bundle, and a
     `README.md` linking the exact `file:line`. Template: `rust/aterm/crates/aterm-spec-models/proofs/ay/a1_row_index/`.
   - *Tier-1/2 in-`trustc`:* for memory safety on a type, mirror `aterm-scrollback`
     (`src/lib.rs:11-13` preamble + `#[cfg_attr(trust_verify, trust::backing)]` `mmap.rs:23`),
     check with `RUSTC_BOOTSTRAP=1 rustup run trust cargo trust check`. For all-inputs functional
     properties add a `#[cfg(kani)] #[kani::proof]` harness (template:
     `aterm-scrollback/src/kani_proofs.rs`, use a state-collapsing stub to dodge CBMC blowup) and
     register it in `verify-kani-proofs.sh`.
   - Scope honestly: an SMT bound proves the *arithmetic* half; a full `get_unchecked` license also
     needs a borrow/aliasing lemma `trustc` may return `Unsupported` for — record it as a held
     precondition rather than overclaiming.
4. **Wire the build.** wasm → add to `config/scripts/build-aterm-wasm.mjs`'s `CRATES` map + a
   `load-*.ts`; track `_bg.wasm`, gitignore the generated `.js/.d.ts`. napi → extend
   `native/orca-node` `#[napi]` methods (+ the TS handle type in `rust-terminal-addon.ts:10-45`) or
   a sibling addon like `build-terminal-addon.mjs`; keep the `.node` gitignored, add the rebuild to
   the relevant `package.json` scripts. Gate proofs: Option-A rides `rust/aterm/tools/verify.sh --full`;
   Option-B needs a new `verify.sh` + `pnpm` script. `bump-aterm.mjs` regenerates both artifact
   families on an engine bump.

### Anchor index for implementers
- Trust cfg + forks: `rust/aterm/Cargo.toml:107,158-169`
- SMT bundle template: `rust/aterm/crates/aterm-spec-models/proofs/ay/a1_row_index/`
- BMC harness template: `rust/aterm/crates/aterm-scrollback/src/kani_proofs.rs`
- Trust annotations: `rust/aterm/crates/aterm-scrollback/src/{lib.rs:11-13,mmap.rs:23,53}`
- Merge gate: `rust/aterm/tools/verify.sh` (proofs `:270-300`)
- The contract: `rust/PROOF_CARRYING_PERFORMANCE.md`
- wasm seam: `config/scripts/build-aterm-wasm.mjs`, `…/aterm/load-aterm.ts`, `rust/aterm/crates/aterm-wasm/src/lib.rs`
- napi seam: `config/scripts/build-terminal-addon.mjs`, `native/orca-node/src/lib.rs`, `src/main/daemon/rust-terminal-addon.ts`, `rust/crates/orca-terminal/src/headless.rs`
- Bump/sync: `config/scripts/bump-aterm.mjs`; new crate registration: `rust/Cargo.toml:14`

---

## 7. Aterm responsiveness roadmap

Making the terminal itself faster/crisper/more responsive. aterm v0.5.x already shipped a
proven typing-latency fix + hot-path alloc trims; the orca-side wins remaining, by leverage:

1. **Run the engine off the renderer main thread.** aterm wasm + the GPU drawer currently
   run on the renderer **main thread** (`load-aterm.ts` — no Worker), so terminal compute
   competes with React/layout/paint. Move the engine + an `OffscreenCanvas` into a dedicated
   Worker; the main thread only forwards input + transfers byte chunks. The single biggest
   structural responsiveness win (isolates terminal jank from UI jank, and vice-versa).
2. **Eliminate the double UTF-8 transcode on the output path.** node-pty decodes bytes→string
   in main, IPC structure-clones the string, the renderer re-encodes to bytes for
   `term.process()` (`aterm-process-pump.ts`). Deliver raw PTY **bytes** via a napi
   transferable `ArrayBuffer` straight into the engine — kills two transcodes + the clone +
   steady renderer GC. (Unlocked by moving OSC scanning into the engine, §8.2.)
3. **GPU-default widening + context-loss hardening + single-copy CPU.** The CPU fallback
   double-copies the framebuffer (`aterm-frame-painter.ts` — `new Uint8ClampedArray(term.rgba())`,
   ~32 MB/frame on Retina). Remove the redundant copy; widen GPU eligibility + harden
   `webglcontextlost` recovery so fewer panes (esp. SSH/RDP/VM/headless) land on CPU.
4. **Predictive (local) echo for high-latency sessions.** Over SSH, echo typed glyphs locally
   before the PTY round-trip (mosh-style), reconciled when the real bytes arrive — hides RTT
   on the keystroke→glyph path that no main-side fix can.
5. **Engine-side micro-wins (upstream into aterm, ride its proof gate):** glyph-atlas warming,
   ligature/shaping caches, damage-region redraw if not already, scrollback mmap tiering
   (already has the `budget_chc` OOM-impossibility proof to extend).

## 8. Trust subsumption order (what to pull in next)

The recipe is proven (orca-git: pure core + `ay` proofs + napi + dual-run parity + cutover
behind fallback). Trust is strongest where the hot core is **pure integer/byte logic**
(parsers, codecs, indexing, budgets, crypto). Ordered by (UX leverage × Trust fit):

1. **✅ git status/diff parser** — DONE (this is §2 #1; live as of 2026-06-28).
2. **Terminal output byte pipeline** (§2 #2) — fold OSC-9999/title/bell scanning into the aterm
   engine (drain structured events instead of 1-3 renderer-thread string scans/chunk); move
   the main-process 8 ms batcher to a Rust byte **ring with a budget invariant**; then the
   byte transport (#2 above). *Option A — inside the aterm submodule, rides its Trust gate for
   free.* Highest remaining leverage (hottest UX path) **and** strongest proof fit.
3. **`orca-text` fuzzy ranking + file-tree projection** (§2 #3) — per-keystroke over the full
   file list on the UI thread → wasm; index once on panel open, rank per keystroke. Proof:
   score-in-range + returned-index-in-bounds. (Collation: needs an ICU-equiv or a documented
   ordering change.)
4. **Full diff/patch engine** — `orca-git` already owns `compute_line_stats`; extend to a
   verified Myers diff + hunk apply (the editor's per-section diff stats + `git apply` paths).
   Proof: index bounds + no-overflow on the LCS/edit-script arithmetic.
5. **`orca-proto` + `orca-crypto`** (§2 #5) — the RPC envelope codec + `crypto_box`/base64 for
   the web/mobile client → wasm. Proof: codec totality, no-overflow, constant-time. Benefits
   the SSH/phone-driving path where all I/O flows through JS crypto today.
6. **Persistence validation** — the narrow, hot, pure parts of the store hydrate (pane-identity
   normalization, JSON validation invariants) → napi. *Not* the 40-migration pipeline (churny,
   low-ROI, high-risk — keep in TS).
7. **Path confinement / gitignore matching** — `orca-core` path logic; aterm already ships a
   `PathConfine` TLA spec to mirror. Proof: no path escapes the worktree root.

Pair every subsumption with the §4 non-Rust wins (locale lazy-load shipped; still pending:
code-split the eager index, `Promise.all` the boot IPCs, move the rg-`--json` parse + the
Electron fs-watcher to workers). Those dominate startup and are *not* Rust work — don't spend
verified-Rust effort on parse-cost a dynamic `import()` fixes.

---

## 9. Aterm-efficiency sprint — outcome + the #5 Worker design

**Landed + measured (2026-06-28):** zero-copy CPU framebuffer (Tier 1 + Tier 2 `rgba_ptr`,
~4–16 ms/frame), `process_str` (no per-chunk transcode alloc/copy), aterm → v0.5.8 (blit-1×1
fast path ~22%, FxHash glyph caches, +2.7% SGR, resize throttle), and the dead per-chunk
`drain_bell` gated out. **Investigated → deliberately unchanged:** the per-chunk OSC/title
scanners are load-bearing for agent-status (OSC-9999, no engine support; runs engine-less in
hidden sessions) and the title→agent-state machine (needs every title in order; engine exposes
only last-per-chunk) — removing them breaks agent detection; the GPU auto-policy is correctly
conservative (rejecting software/unknown renderers avoids render-corruption — widening
re-introduces it; the real GPU gains live in the engine, banked via v0.5.8).

**Reassess #5 before building it:** the audit measured per-frame cost *with* the framebuffer
copies present. Those copies (the dominant main-thread per-frame cost) are now gone, so the
Worker move's marginal responsiveness benefit is smaller than first estimated. **Measure the
residual main-thread per-frame cost (CDP Performance trace under a `cat` flood) first** — if it's
already small, #5's XL cost may not pay for itself.

### #5 — engine off the main thread (Worker + OffscreenCanvas), sequenced

The blocker is the facade's *synchronous* surface. Split it:
- **Cacheable state** (mutates only on process/scroll/resize): cols/rows, `display_offset`,
  cursor x/y, `baseY`, title, `isAltScreen`, bracketed-paste/mouse/focus modes, `cellSizeCss`.
  The worker pushes a snapshot after each process; the main facade reads the cache synchronously.
- **Query reads** (parameterized/large): `serialize`/`serializeScrollback`, `rowText`/`cellText`/
  `rowLen`/`rowIsWrapped`, `selectionText`, `findMatches`/`searchActiveMatchRect`, `linkAt`. These
  are **interactive** (snapshot-save, copy, search, hover hit-test) — *not* per-frame — so they
  tolerate an async worker RPC.

Staged rollout (each stage independently shippable + verifiable):
1. **Async-ify the interactive query call sites** (add `Promise` variants alongside the sync ones;
   migrate the ~consumers one at a time, sync stays until all migrated). Non-breaking; the real
   "80%". Gate each migration with the existing tests.
2. **Worker render module** (`aterm-render-worker.ts`): holds the engine, takes the
   `transferControlToOffscreen` canvas, `process_str` via postMessage, renders to the
   OffscreenCanvas (CPU first; GPU/webgl2-in-worker second). Wire as an **opt-in strategy** beside
   the current one (default off) so production is unaffected.
3. **Cacheable-snapshot protocol** worker→main (postMessage, or a `SharedArrayBuffer` ring for
   zero-latency sync reads + `Atomics` for the query RPC if async proves too laggy on hover).
4. **Flip the default** only after e2e (Electron/CDP) shows input→glyph latency unchanged AND
   UI-jank-during-flood improved vs the now-copy-free main-thread path.

Alternative considered + deferred: a single **shared-memory wasm build** (wasm threads + COOP/COEP
cross-origin isolation) would let main read engine state synchronously from the same linear memory
while the worker renders — elegant, but it's a large engine build change (threaded wasm) + an
Electron isolation requirement, so it's a bigger lift than the snapshot+async-RPC path above.

### #7 — predictive/local echo (deferred)
Niche (SSH/high-RTT only), mosh-class complexity (reconciliation, password/TUI suppression, cursor
prediction), high visual-glitch risk. Lowest priority; only worth it once #5 is done.
