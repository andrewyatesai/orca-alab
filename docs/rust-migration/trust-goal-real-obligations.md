# Trust goal — prove REAL obligations, fail-closed on vacuity (no vanity metrics)

> Paste the block below as the session goal / standing directive for the agent
> working on Trust (`~/trust`), using Orca (`~/orc/rust`) as the workload. It
> supersedes `trust-improvement-goal.md`, which was satisfiable by vanity metrics.

---

**The hard lesson that defines this goal (2026-06-08).** A previous run reported
"orca-core: 244 proved" and celebrated it. It was bullshit. ALL 244 were a
synthetic per-function placeholder — `trust_mc_default_function`, predicate
`bool_literal(false)`, injected by `ensure_default_trust_mc_function_obligation`
(`compiler/rustc_mir_transform/src/trust_verify.rs:3398`) and mislabeled
`kind: ArithmeticSafety`. It proves **regardless of whether the function is safe**:
it proved for `unbounded(a,b) = a + b` whose *real* overflow obligation correctly
**failed**. The real safety obligations of orca-core (≈2500: arithmetic overflow,
asserts, preconditions, bounds, unsafe-op) were and are **all `unknown`**.
**Zero real safety obligations prove on orca-core.** Trust was manufacturing false
confidence and would have silently "verified" buggy code.

**Mission.** Make Trust prove the **real, non-vacuous** safety and correctness
obligations of real-world Rust (Orca) — and **fail-closed (refuse to compile) on
anything vacuous, trivially true, or unverified.** Trust is only useful if
"verified" means a *falsifiable* property was actually *proved*, and if Trust
*errors* on code whose safety it cannot establish. Green checkmarks for unproven
or vacuous obligations are worse than no verifier.

**The ONLY metric that counts.** Real safety/correctness obligations **proved
non-vacuously**: integer overflow/underflow, array/slice bounds, division-by-zero,
panic-freedom (`panic!`/`unwrap`/`expect`/`assert!`/indexing unreachable), and user
contracts (`#[trust::requires/ensures]`). **Never counted:** the
`trust_mc_default_function` admission, any obligation with a literal `true`/`false`
goal, any obligation whose assumptions are UNSAT, "the function lowered", "the
bundle was admitted", or any synthetic/pipeline marker. If a function contains an
operation that *can* fail, the matching obligation MUST exist and be PROVED — or
the function is NOT verified. Report counts **by real kind**, never a single
"proved" total.

**Vacuity detection + fail-closed — the core new requirement (what the user
demanded: "detect these and fail to compile vacuous statements").**
1. **Kill the vanity obligation.** Stop minting `trust_mc_default_function` as a
   proof; tag it non-safety and exclude it from every count, verdict, JSON row, and
   report. A function with only that "proof" verified nothing.
2. **Vacuity gate on every `proved` obligation.** Standard model-checking vacuity
   (Kupferman–Vardi): an obligation is a *real* proof iff `assumptions` is SAT and
   `assumptions ∧ ¬goal` is UNSAT. If `assumptions` is UNSAT (antecedent
   unsatisfiable → anything follows), or `goal` is a syntactic `true`/`false`
   literal, the "proof" is **vacuous → downgrade to a hard failure** and emit a
   diagnostic. Apply at the result-classification site (`convert_result`,
   `trust_verify.rs:~8244`) and the verdict (`summarize_verdict`, `:~6980`).
3. **Falsification self-test (mutation) — the proof that Trust is useful.** A
   "proved" claim is credible only if a buggy variant is **refuted**. For each real
   obligation Trust proves, a self-test must confirm that a mutation violating it
   (delete the guard, widen the bound, drop the precondition) flips it to `failed`.
   If a mutation does not change the verdict, the original proof was vacuous — fail.
   Bake `bounded`✓/`unbounded`✗-style controls into the verification harness and run
   them on **real Orca functions**, not toy probes.
4. **Fail-closed verdict.** A function is `Verified` only if: ≥1 real obligation,
   **all** real obligations `proved` non-vacuously, **zero** `unknown`/`failed`/
   `timeout`. Any `unknown` real obligation ⇒ `Inconclusive` (NOT verified). Under
   `tcargo trust check` / `-Z trust-verify-full`, `Inconclusive` or any vacuous
   proof is a **compile error** (non-zero exit).

**Definition of done.** For `orca-core`, `orca-text`, `orca-config`, `orca-agents`:
every real safety obligation is **proved non-vacuously** (per-function JSON shows
the real kinds — overflow/bounds/panic/asserts/contracts — as `proved`, the vanity
admission absent); the `unknown` count for **real** obligations reaches **zero**;
AND the mutation self-test passes (each proved obligation shown falsifiable); AND a
CI gate runs it and goes **red** on any `unknown`, vacuous proof, or surviving
mutation. "Done" is never "N proved" — it is "every real obligation
proved-and-falsifiable, build red otherwise."

**The actual lowering work (why real obligations are `unknown` today).** A
function's real obligations are only *checked* if the whole function LOWERS to
TrustIr. orca-core's real functions still contain un-lowered calls/ops — str
methods (`to_lowercase`/`find`/`split`/`chars`), `Box::new_uninit`, `Option::map`,
`Try::branch` (`?`), iterators, `Deref`, `&str` literals — so they report
`Unsupported` and their overflow/assert/precondition obligations never run. Finish
the lowering, **but gate every increment on a real obligation proving on a real
function AND a buggy variant of that function failing.** Lowering that only flips
the vanity admission to "proved" is worthless; ignore that counter entirely.

**Anti-self-deception (re-read every iteration).** The previous run fooled itself
by tracking total "proved" instead of real-kind "proved", and never ran a
buggy-variant control on the target. Rules: (a) report ONLY real-obligation counts
by kind; (b) for every "proved" claim, exhibit the matching buggy variant
`failed`; (c) distrust any all-green result until a mutation breaks it; (d)
`unknown` is not progress; (e) the vanity admission does not exist as far as
metrics are concerned; (f) when in doubt, assume you are measuring the wrong thing.

**Operational constraints (unchanged).** Disable the sandbox for builds/network
(`dangerouslyDisableSandbox`); incremental compiler rebuild ≈ 20 min (batch
changes, prefer no-rebuild probes); always rebase onto latest `origin/main`;
private-origin push only (no public release/mirror); soundness AND honesty are the
hard line — never report a proof you have not shown to be real and falsifiable.

**First moves.** (1) Make `unbounded(a,b)=a+b` cause `tcargo trust check` to FAIL,
and prove the failure is because the real overflow obligation is unproved — not the
vanity admission. (2) Implement the vacuity gate + verdict so a function with only
`trust_mc_default_function` is `Inconclusive`, and re-survey orca-core reporting
**real** obligations only (expect ~0 proved — that is the honest baseline). (3)
Then resume lowering, each step validated by a real obligation proving on a real
orca-core function and its mutant failing.

---
