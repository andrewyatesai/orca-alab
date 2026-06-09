# Goal prompt — make Trust prove real-world Rust (Gaps 3 & 4 + toolchain features)

> Note (2026-06-08): when interpreting "obligations proved (not unknown)" below,
> count only REAL, falsifiable obligations — a vacuous per-function placeholder was
> found to "prove" regardless of safety. See the companion
> [`trust-goal-real-obligations.md`](./trust-goal-real-obligations.md) for the
> real-obligations discipline (vacuity detection, fail-closed, mutation self-test).

> Paste the block below as the session goal / standing directive. It is written
> to drive an autonomous agent, using Orca as Trust's proving ground.

---

**Mission.** Make **Trust** (your verifying Rust compiler fork at `~/trust`, private
remote `andrewyatesai/trust`) able to **actually prove the obligations of
real-world Rust**, using the **Orca** workspace (`~/orc/rust`) as the canonical
verification workload. Drive the co-evolution loop: run Trust on Orca → every
`unknown` / `unsupported` / pipeline failure is a Trust ticket → fix Trust →
rebuild → re-verify → repeat, until Orca's pure-logic crates verify clean.

**Why.** Orca is the flagship app on a fully-owned verified stack. Trust currently
*builds and runs* on Orca (first-party verification fires after the genesis-flag
and local-scope fixes already on `origin/main`), but it cannot yet *prove* real
obligations. Closing that turns "604 passing tests" into "machine-proved
panic/overflow/contract safety" — the moat.

**Current state (already done; build on it):**
- Trust stage2 builds; `trustc 1.96.0-dev` runs; Orca crates compile under it.
- Fixed & pushed: (1) genesis stage0 wrapper strips `-Z*trust*` flags; (2) local
  MIR is always in verification scope (was skipped as `ExternalDependencyScope`).
- Orca carries inert-under-stock-cargo Trust contracts (`#[cfg_attr(trust_verify, trust::ensures(...))]`),
  e.g. `orca-agents::agent_status_types::truncate_preserving_surrogates`.

**Objectives (priority order):**

1. **Gap 3 — TrustIr lowering of core/std call targets (THE blocker).** The full
   verifier cannot lower calls to core/std functions into TrustIr — even
   `u32::wrapping_add` or a derived `Clone` returns `unknown`
   ("Call target ... is not present in the TrustIr module; use
   lower_to_trust_ir_functions for multi-function lowering"). Implement TrustIr
   lowering and/or a **modeled-summary library** (axioms/contracts for core/std
   intrinsics and common functions) so obligations that call them can be
   discharged. Start with the smallest set Orca actually needs: integer
   arithmetic (`wrapping_*`/`checked_*`/`saturating_*`), slice/`Vec`/`String`
   indexing & length, `Option`/`Result` combinators, derived `Clone`/`PartialEq`.

2. **Gap 4 — `tcargo trust check` pipeline scope.** Direct `trustc` verifies local
   crates and emits `TRUST_JSON`, but `tcargo trust check` reports
   `transport:missing-json` / "1 functions" (a synthetic transport probe), i.e.
   it isn't running per-function verification on the target crate. Make the check
   pipeline pass the local/full verification policy to its trustc subprocess and
   aggregate real per-function `TRUST_JSON` rows into the report + JSON output.

3. **Survey (warning) mode that doesn't abort.** `-Z trust-verify-full` is
   fail-closed and aborts on the first unproved obligation. Provide a crate-wide
   survey that reports every function's proved/unknown/failed counts (with the
   reason for each `unknown`) without aborting — so coverage is measurable as
   Gap 3 lands incrementally.

4. **Contract end-to-end.** Make `#[trust::requires/ensures]` prove on real crates
   (activate via the established `trust_verify` cfg), and prove Orca's seeded
   contracts (start with the UTF-16 length-bound on `truncate_preserving_surrogates`).

5. **CI-grade reporting.** `tcargo trust check --format json` emitting stable
   per-function rows (function, obligation, kind, outcome, reason) that a CI gate
   and Orca's `docs/rust-migration/trust-verification.md` gap log consume.

6. **Reproducible bootstrap.** Fold the build recipe into Trust so a fresh
   checkout bootstraps in one command, auto-checking/installing prereqs
   (cmake/ninja) and initializing submodules over the available credential. Keep
   `download-ci-llvm` choice configurable.

7. **`ty` as the domain-spec layer.** Use the (real, ~19k-file) `first-party/ty`
   submodule as the home for reusable verified specs Orca's crates import.

**The loop (each iteration):**
1. `cd ~/orc/rust && ~/trust/build/host/stage2/bin/tcargo trust check -p <crate> --format json` (or direct `trustc` per-crate while Gap 4 is open). Force recompile so trustc re-runs.
2. Triage outcomes: `proved` → keep; `unknown`/`unsupported` → read the reason, it names the missing MIR op / call target = the next Trust change.
3. Implement the Trust change on the **latest** `origin/main` (fetch + rebase first).
4. Rebuild stage2 (LLVM is cached after the first build), re-verify, update the gap log in `~/orc/docs/rust-migration/trust-verification.md`.
5. Commit Trust fixes onto latest remote main and push (private origin only).

**Definition of done.** Orca's pure-logic crates (`orca-core`, `orca-text`,
`orca-config`, `orca-agents`) verify via `tcargo trust check` with their
panic/overflow/bounds **and** seeded contract obligations **proved** (not
`unknown`), reported per-function as machine-checked JSON, runnable as a CI gate.
Track progress in the gap log; "done" is when the gap log's `unknown`/unsupported
rows for those crates reach zero.

**Operational constraints (learned the hard way):**
- **Disable the sandbox** for all builds/network/installs (`dangerouslyDisableSandbox`); it is NOT an environment limit, the sandbox just blocks them.
- Initial stage2 build ≈ 28 min (LLVM from source, `download-ci-llvm=false`); incremental **compiler rebuilds ≈ 20 min** (LLVM cached). Batch changes; minimize rebuild cycles; prefer no-rebuild diagnostics (`TRUST_DYN_PROBE=1`, direct `trustc` flag probes).
- **Always base Trust work on the latest `origin/main`** (fetch + rebase before changing/committing).
- Trust's `INSTALL.md` forbids **public** upload/mirror/release tags; pushing bugfixes to the **private** origin/main is fine.
- Build recipe: `brew install cmake ninja`; `python3 scripts/recreate_bootstrap.py --stage 2`; submodules via `git config --global url."https://github.com/".insteadOf "git@github.com:"` + `git submodule update --init --recursive` (uses the `gh` token; no SSH key present); `./x.py build --stage 2`.

**First moves:** reproduce Gap 3 with the smallest case (`/tmp/probe.rs` calling
`wrapping_add`), read `compiler/rustc_mir_transform/src/trust_verify.rs` +
`first-party/trust-ir` lowering + `lower_to_trust_ir_functions`, and land core
arithmetic lowering first — then re-verify `orca-core` and watch the `unknown`
count drop.

---

## Progress (2026-06-08)

- **🟢 BREAKTHROUGH (2026-06-08): the artifact-backed admission path ("path B") is
  DONE — it was an identity-string bug, not multi-month core research.** The native
  full-verifier route refused every obligation because the compiler emits the suite
  token as crate-name `trust-mc` (hyphen) while trust-mc native ids use `trust_mc`
  (underscore); three+1 comparison gates compared the raw strings and never matched.
  Fixed (separator-canonicalized, sound). Rebuilt stage2 now **proves QF_LIA
  arithmetic-safety obligations end-to-end** with full proof-grade evidence
  (PdrInvariant + transcript + replay + checked-report, assurance=Sound), and
  correctly **fails** unprovable ones. **orca-core full-mode: 0 → 167 proved
  obligations, 142/697 functions fully proved, 0 failed.** Commits: trust-bmc
  `ade0610b51`, trust-mc-core submodule `eaca4b299`.
- **Obj 1 (Gap 3, core lowering): now the PRIMARY tractable lever (no longer
  multi-month).** With admission working, each lowered family converts unknowns
  directly to `proved`. `wrapping_{add,sub,mul}` lower; the dominant remaining
  blocker is the TrustIr **bridge** failing to lower core/std **call targets**
  (`Clone`/`Default`/`Deref`/`ToString`/iterator…) + the **address-of-field
  projection** MIR op (~2805 of 3074 unknowns). See `trust-verification.md`.
- **Obj 2 (Gap 4, pipeline scope):** ✅ FUNCTIONAL — `tcargo trust check -p orca-core`
  runs per-function (697 fns / 3346 obligations), not a synthetic probe.
- **Obj 3 (survey mode):** ✅ DONE — `--survey` flag + `TRUST_VERIFY_SURVEY`
  (artifact-backed full, non-aborting per-function coverage).
- **Obj 4 (contracts end-to-end):** scaffolded (`#[cfg_attr(trust_verify, trust::ensures)]`
  inert under stock cargo) but proving them is blocked on Gap 3.
- **Obj 5 (CI-grade JSON):** ✅ FUNCTIONAL — `--format json` emits a per-function
  `functions[]` array (`{function, summary, obligations}`).
- **Obj 6 (one-command bootstrap):** ✅ DONE — `scripts/dev-bootstrap.sh`
  (idempotent; auto-installs cmake/ninja, inits submodules via gh token, builds
  stage2 if absent, smoke-tests; `--check` mode). Committed `8da075563b`.
- **Obj 7 (`ty` domain-spec layer):** TODO.

**Net:** the reporting/CI machinery (Obj 2/3/5) works; "zero unknown" is gated on
Obj 1 (Gap 3 core verifier work). Committed: scope/genesis fixes (Trust main),
`wrapping_add`, survey mode, `recheck_cleancic`, `--survey` flag (branch
`trust-gap3-wrapping-add`).

## Progress (2026-06-09, builds #19–#31) — supersedes the 2026-06-08 block above

Full detail in [`trust-verification.md`](./trust-verification.md) (gap log) and the
`trust-verifying-compiler` memory. Current accurate state:

- **Obj 1 (Gap 3 lowering): the analyzability frontier is largely cleared; the count
  is now gated by CONTRACTS, not lowering.** Landed + mutant-validated sound:
  u64/usize add/sub overflow (BV re-encode, 0→26 proved); reference-to-aggregate
  constants (`&[&str]` tables → OpaqueConst, 1364→1280); `str::*` total summaries;
  `Deref`/`ToString`/`Try::branch` (`?`) type-gated totals; cleanup-edge drop;
  **NoOverflow predicates** (all 8 BV overflow predicates — signed div/neg/add/sub/mul
  — exact two's-complement expansions in `typed_chc_ay.rs`; `safe_div`/`safe_neg`
  PROVE, mutants REFUTED); **Tier-1 call-peel** (Option/String/Vec/slice structural
  accessors + primitive predicates as fresh-symbolic). These made orca-core's
  arithmetic/derived obligations ANALYZABLE — but they now correctly REFUTE for
  extreme inputs (e.g. `days_from_civil`'s `year_of_era*365`, timestamp deltas), so
  PROVING them needs preconditions (→ Obj 4). Remaining HARD lowering frontier:
  `Unsize`→`&dyn` (274, #1), closure-bearing combinators (~150, unsound to fake),
  `Box::new_uninit`/`Formatter` (~130), and `TyKind::Alias` projection-normalize
  (111 — DEFERRED: 4 attempts incl. the `adt_arg_depth` guard all hit E0275
  trait-solver overflow on typenum/zlib-rs, even during the stage2 build).
- **Obj 2 (Gap 4): ✅ DONE + DETERMINISTIC.** `tcargo-trust` now force-cleans the
  target package before verify so trustc re-emits per-function `TRUST_JSON` every
  run (the `transport:missing-json` degradation was an intermittent cargo cache hit).
  A survey yields a 7.8 MB per-function/per-obligation JSON with exact blocking
  reasons.
- **Obj 3 (survey mode): ✅ DONE** (`TRUST_VERIFY_SURVEY=1`, non-aborting per-function
  proved/unknown/failed + reason).
- **Obj 4 (contracts): SCOPED precisely; the schema-decode layer is cleared, the
  body-assumption capability remains.** Fixed the trust-wp formula-schema separator
  bug (`trust_wp.` underscore emitted vs `trust-wp.` hyphen decoded) → derived-PartialEq
  preconditions now DECODE and reach the prover. But a `#[trust::requires(P)]`
  predicate still reaches trust-mc as a `router_placeholder` (`predicate=false`),
  rejected by the "not MIR-derived; router placeholders are not proof input" gate —
  so P never constrains the body. **THE remaining Obj-4 capability:** lower the
  requires predicate into a MIR-derived typed CHC and CONJOIN it as a body-assumption
  hypothesis. Contract opt-in confirmed: `#![feature(register_tool)]` +
  `#![register_tool(trust)]` + `--cfg trust_verify`.
- **Obj 5 (CI-grade JSON): ✅ DONE** (deterministic per-function rows; removed a stray
  `DEBUG: prove_native_pure_predicate` stdout print that polluted `--format json`).
- **Obj 6 (bootstrap): ✅ DONE** (`scripts/dev-bootstrap.sh`).
- **Obj 7 (`ty` domain-spec layer): TODO.**

**Honest count (orca-core, build #31):** 1280 obligations, 0 publishable-proved (the
hardened gate requires solver-transcript artifacts the BV route doesn't yet emit —
a separate sound future win), ~1209 unknown + 71 failed (67 of the "failed" are
vacuous `predicate=false` placeholder preconditions exposed by the schema fix —
should be excluded from the honest count, same class as the `trust_mc_default_function`
admission). **Reaching zero unknown across all four crates is a multi-session program;
the validated next lever is the contracts body-assumption capability (Obj 4), then
the hard lowering frontier (Unsize/dyn, closures).** All changes uncommitted on the
working tree (NoOverflow, schema fix, Tier-1, constants, Gap-4); nothing pushed.
