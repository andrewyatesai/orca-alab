# Capability gaps exposed by the hand-coded-SMT purge

**Status:** open asks against the `trust-*` stack, written 2026-08-08.
**Audience:** whoever works on `trust-ir`, `trust-vcgen`, `trust-mc`, `trust-wp`, `ay`.

## Why this file exists

Eight first-party crates under `rust/crates/` carried hand-written SMT-LIB2 proof
bundles in `proofs/ay/`. They have been deleted. This file is the other half of
that deletion: the properties those bundles *claimed* to cover that the `trust-*`
stack cannot express today, stated concretely enough to be implemented against.

**Do not read this as "the proofs were wrong to want these properties."** The
properties are real and the code still depends on them. What was wrong was the
mechanism: a hand-written model, maintained by hand, next to the code, with
nothing forcing the two to agree.

### The specific failure that motivated the purge

The bundles modelled decision logic in `QF_LIA` over **unbounded `Int`**.
`rh1_band_bound` opens with `(declare-const t Int)`. Machine-integer overflow is
therefore outside every one of them by construction — not missed, but *unstateable*.
Five reachable `i64`/`u64` overflow bugs sat underneath a green
`ALL PROOFS DISCHARGED` banner until the compiler's verifier refuted them
(fixed in `d933238fa`). A model that cannot express the bug class it is guarding
against reports success precisely when it is least useful.

The same shape recurs throughout the retired bundles:

- `bo1_throttle_bound` proved `min(30000*p, 900000) ∈ [30000, 900000]` for
  unbounded `p`. The code computes `ACTIVE_FAILURE_REFETCH_MS.saturating_mul(1u64 << exp)`.
  The interesting question — what happens at `exp >= 64` — is not in the model.
- `line_scan_in_bounds` **assumed** `start <= nl < len`, which is exactly memchr's
  postcondition and the only hard part, then proved the trivial remainder.
- `cap_buffer_le_limit_plus_2` postulated "a line pushes at most 2 entries" and
  "entering a line, `count <= limit`" — both claims about the parser's control
  flow that the model asserted rather than checked.
- `decode_octal_mask_matches_uint8` proved `(v & 0xFF) == v mod 256` for
  `v <= 511`. True of any `v`, and connected to the decoder only by a comment.

Every bundle was linked to the code it described **by prose**. That link is what
broke, and no amount of solver rigor downstream of a hand-written model repairs it.

### What replaced them

| Route | Meaning |
|---|---|
| **Compiler** | The obligation is emitted and discharged by `-Ztrust-verify=on` on the real MIR and the real machine types. Nothing to maintain. |
| **In-source test** | An exhaustive or class-exhaustive property test running the *real* function. Strictly stronger than the retired model, which ran nothing. |
| **Gap (this file)** | The stack cannot express it today. Recorded here, covered by a test in the meantime. |

The regression guard lives in `tools/terminal-bench/gauntlet-certificates.mjs`
(the `certificates` gauntlet axis): any `.smt2` or `.alethe` file reappearing under
`rust/crates/` FAILs the axis, with `tools/terminal-bench/gauntlet-certificates.test.mjs`
planting violations to prove the guard actually fires.

## Measurements

All figures below are from `trust/build/host/stage1/bin/trustc`
(`rustc 1.99.0-dev (a46085a8e 2026-08-07)`) at
`-Ztrust-verify=on -Ztrust-verify-function-budget-steps=100000`, single-file,
2026-08-08. They are what the stack does *now*, not what it is documented to do.

| Unit | Obligations | proved | failed | unknown | runtime-checked |
|---|---|---|---|---|---|
| `orca-provider-backoff` (whole crate) | 1 | **1** (kernel-certified) | 0 | 0 | 0 |
| `orca-stream-split` (whole crate) | 2 | **2** (kernel-certified) | 0 | 0 | 0 |
| `orca-renderer-heap` (whole crate) | 1 | 0 | 0 | 0 | 1 |
| `NdjsonSplitter::feed` | 1 | 0 | 0 | 0 | 1 |
| `decode_git_cquoted_path` | 26 | 0 | 0 | 1 | 25 |
| status-scan loop (`P::update` extract) | 9 | 0 | 5 | 1 | 3 |

The pattern is sharp and worth stating plainly: **the stack proves pure
machine-integer arithmetic and proves it well** — those two kernel-certified
crates are genuine, and they are exactly the crates whose retired bundles were
most redundant. Everything touching `Vec`/`String`/slices falls to
runtime-checked or spurious refutation. Gaps S1 and C1 below are why.

---

## S1 — container length is opaque, so no index is related to its buffer

**Property that needs it.** `&text[start..end]` and `text[nl - 1]` in
`orca_git::status_stream::update` are in bounds; `chars.get(i)` in
`orca_core::git_cquoted_path` covers its domain.

**What happens today.** `Vec::len()` / `String::len()` are absent callees, so the
verifier has no fact connecting the returned `usize` to the buffer's actual
extent. Measured on the scan loop: 0 of 9 obligations proved, and two `[slice]`
obligations **refuted** with counterexamples at `start = nl = rel = 0` — states
that cannot occur, produced because nothing says `len(text) > 0` there. The
source already carries scar tissue from this; `git_cquoted_path.rs` opens with a
comment explaining that `n` is "an opaque value the verifier cannot relate back to
the buffer, so an `i < n` guard proves nothing about the index."

Note the failure mode: **spurious refutation**, not silence. At
`-Ztrust-policy=strict` this makes correct code un-buildable, which is why the
workspace sits at `advisory` — and `advisory` is not a gate.

**Why the retired SMT did no better.** It assumed the relation it needed
(`start <= nl < len`) as a precondition. Same missing fact, hidden in a
`(assert ...)` instead of surfaced as a refutation.

**What would close it.** Model core container length as an uninterpreted function
with the axiom tying `len(v)` to the slice metadata the MIR already carries, so
`v[i]` with `i < len(v)` discharges. Concretely: `Vec`, `String`, `str` and slice
`len`/`get`/`index` need to stop being absent callees — either via the Trust-Std
bundle or via built-in models in `trust-vcgen`. This is the single
highest-leverage item in this file; it alone would move most of the 25
runtime-checked obligations in `decode_git_cquoted_path`.

## C1 — no way to state a callee's postcondition

**Property that needs it.** `memchr::memchr(0x0A, &text[start..])` returns
`Some(rel)` only when `rel < text.len() - start`. Every index bound in the status
scan loop descends from this one fact.

**What happens today.** Nothing states it. `memchr`'s SIMD body is not in the
lowered bundle, so the call is an absent callee and its result is unconstrained.
The retired `line_scan_in_bounds` bundle handled this by *assuming* it in a
`(assert (bvule start nl))`, which is a documented assumption dressed as a proof.

**What would close it.**

1. `#[trust::ensures(...)]` honored **across crate boundaries**, so a contract
   written once on a function is usable by every caller.
2. A way to attach a contract to a **third-party function you cannot edit** — an
   extern-contract/assumption file the verifier reads, so the assumption is
   explicit, greppable and auditable instead of living in a comment. An assumption
   the tool knows about can be listed in a TCB inventory; one in a comment cannot.

Until then, first-party `memchr` (task #11) also closes this particular instance
by making the body available for lowering.

## I1 — no struct/state invariants, only per-call panic obligations

**Property that needs it.** The OOM bound:
`NdjsonSplitter::buffer.len() <= max_line_bytes` after **any** `feed`, across
statement boundaries and across calls. This is the entire reason the splitter
exists on an untrusted socket.

**What happens today.** Trust's Level 0 emits panic/UB obligations per operation.
An inductive invariant over a struct's fields, preserved by every public method,
is not in that vocabulary. Measured: `feed` raises exactly one Level-0 obligation
and it is runtime-checked. The retired `oom_buffer_le_max` bundle reasoned about a
single `push_str` in isolation under an assumed guard, with the surrounding
control flow supplied by a prose comment — it never covered the inductive step
either, which is precisely what makes this a bound rather than a one-line fact.

**What would close it.** `#[trust::invariant(self.buffer.len() <= self.max_line_bytes)]`
on the type, discharged as an inductive invariant: assumed at every public method
entry, proved at every exit. `trust-mc` can already do the bounded version (BMC to
depth *k*, which would catch a badly-placed guard); `trust-wp` is the right home
for the unbounded inductive proof. Depends on S1 to say anything about `len()`.

Covered meanwhile by
`orca_net::ndjson::tests::buffer_never_exceeds_budget_on_any_chunking`.

## R1 — relational (2-safety) properties are not expressible

**Property that needs it.** Monotonicity, in three places:
`t1 <= t2 => clamp(t1) <= clamp(t2)` (renderer heap ceiling),
`streak1 <= streak2 => throttle(streak1) <= throttle(streak2)` (backoff), and the
keep-tail clamp in `orca-flow-control`.

**What happens today.** Every obligation is a property of a *single* execution. A
statement about two executions of the same function cannot be phrased at all. The
retired `rh2_clamp_monotone` / `bo2_monotone` bundles could state it only because
they had abandoned the code and were writing free-standing formulas.

**What would close it.** Self-composition / product-program support in
`trust-vcgen`: `#[trust::relational(forall a b: a <= b ==> f(a) <= f(b))]` lowered
by instantiating the function's VC twice over renamed state and conjoining. This
also buys determinism and non-interference properties, which the parity corpora
currently approximate by sampling.

Covered meanwhile by the sweep tests in `orca-renderer-heap` and
`orca-provider-backoff`.

## F1 — no floating-point theory, and float→int casts raise nothing

**Property that needs it.** `orca-renderer-heap` computes
`floor(total_bytes / 2^30 * 0.4) * 1024` in `f64` and then casts to `u32`. The TS
production sizing must compute the identical value in JS `Number`.

**What happens today.** The `f64` pipeline raises **no obligations at all** — the
verifier is silent about it, which reads like success and is not. The whole crate
raises one obligation, runtime-checked. The retired `rh1_band_bound` bundle
"handled" floats by declaring the computed target a free unbounded `Int` and
reasoning about the integer clamp around it, i.e. by deleting the float layer from
the model and proving something about the leftovers.

**What would close it.** `QF_FP` lowering in `trust-ir` + `trust-vcgen`, plus
obligations for float→int casts under Rust's saturating-cast semantics, and for
NaN/infinity propagation. `ay` already speaks SMT-LIB `FloatingPoint`; the gap is
in the lowering, not the solver.

Covered meanwhile by `parity-corpus.txt` (a differential oracle against the real
TS, which is the right tool for *equivalence* even once F1 lands) and by
`ram_tier_is_banded_and_never_falls_as_ram_rises`.

## N1 — no vacuity or reachability reporting

**Property that needs it.** Every retired bundle paired each `unsat` theorem with
a `sat` control asserting the theorem was not vacuous — `bo_c1` proved the
un-saturated branch was reachable, `cap_nonvacuity_sat` proved `limit + 2` was
actually attained, `oom_catches_unguarded_sat` proved the guard was load-bearing.
**This discipline was the genuinely good idea in the retired bundles** and it has
no equivalent in the stack today.

**What happens today.** Trust reports proved / failed / unknown / runtime-checked.
An obligation that is vacuously true because its path condition is unsatisfiable
is reported as **proved**, indistinguishably from one that is proved over a
reachable domain. A dead branch and a verified branch look the same.

**What would close it.** A `-Ztrust-report-vacuous` mode: after discharging an
obligation, check the path condition for satisfiability and flag the obligation
when it is UNSAT. Cheap — the solver call is already set up and the query is
strictly smaller than the one just run — and it converts a whole class of silent
non-results into visible ones.

Covered meanwhile by explicit `saw_*` counters in each replacement test
(`assert!(saw_cap > 0, "the cap is never active — the clamp is vacuous")` and
friends), which is the same discipline done by hand.

---

## T1 — a `tippy` lint for hand-coded solver logic (NOT YET IMPLEMENTED)

**Ask.** `tippy` should warn when Rust source hand-rolls solver input or drives a
solver directly, the same way this purge banned it at the file level.

**Why the repo guard is not enough.** The
`tools/terminal-bench/gauntlet-certificates.mjs` guard catches `.smt2`/`.alethe`
**files**, which is the form the purged bundles actually took. It cannot see the
other form: SMT-LIB built as Rust string literals and piped to a solver at
runtime. That variant already exists in-tree
(`rust/aterm/tools/temporal-extract/refine-smt/`), so the pattern is live, and it
is the form most likely to come back after a file-level ban.

**Proposed lint: `hand_coded_smt`** (style, warn-by-default), firing on:

1. A string literal (or `format!`/`write!` template) containing an SMT-LIB
   command token at s-expression head position — `(set-logic`, `(declare-const`,
   `(declare-fun`, `(assert`, `(check-sat`, `(get-model`.
2. `Command::new` / `Command::new(..).arg(..)` where the program resolves to a
   known solver name — `ay`, `z3`, `cvc5`, `yices`, `boolector`.

**Message.** *"hand-coded SMT bypasses the verifier that already checks this code.
Express the property as a contract or an in-source property test; if the
`trust-*` stack cannot express it, record a capability ask rather than modelling
the code by hand."*

**Allow-listing.** `#[allow(hand_coded_smt)]` with a required reason, for the
places where driving a solver **is** the product — `trust-mc`, `ay`'s own tests,
`refine-smt`. The lint's value is that those become an explicit, greppable list
instead of an unbounded practice.

**Not implemented here.** `tippy` lives in the Trust repo
(`trust/src/tools/tippy`, a clippy fork), so this needs a `declare_clippy_lint!`
+ `LateLintPass` there and a toolchain rebuild — a change to the verifier's own
tooling, which belongs in that repo's review flow, not smuggled in alongside a
consumer-side purge.

## Deletion inventory

| Crate | Retired bundle | Route |
|---|---|---|
| `orca-provider-backoff` | `bo1`, `bo2`, `bo3`, `bo_c1`, `bo_c2` | Compiler (1 obligation, kernel-certified) + `backoff_band_monotone_and_saturating_over_every_streak`; R1 |
| `orca-stream-split` | `cs1`, `cs2`, `ns1`, `ns2`, `cs_c1`, `ns_c1` | Compiler (2 obligations, kernel-certified) + exhaustive class-sequence tests |
| `orca-net` | `oom_buffer_le_max`, `oom_no_wrap`, `oom_nonvacuity_sat`, `oom_catches_unguarded_sat` | `saturating_add` makes `oom_no_wrap` moot at the source; rest → `buffer_never_exceeds_budget_on_any_chunking`; **I1** |
| `orca-git` | `cap_emit_le_limit`, `cap_buffer_le_limit_plus_2`, `cap_nonvacuity_sat`, `cap_catches_false_tight_sat` | `cap_bounds_the_emitted_and_the_buffered_entry_counts`, `a_zero_limit_disables_the_cap` |
| `orca-git` | `line_scan_in_bounds`, `line_scan_nonvacuity_sat`, `line_scan_catches_false_strip_sat` | `line_scan_handles_every_terminator_shape_at_every_chunk_boundary`, `a_bare_cr_record_neither_underflows_nor_emits`; **S1 + C1** |
| `orca-git` | `decode_octal_total`, `decode_octal_mask_matches_uint8`, `decode_octal_wrap_reachable_sat` | `octal_escape_decode_is_total_over_every_escape` (exhaustive, all 584 escapes); **S1** |
| `orca-renderer-heap` | `rh1`, `rh2`, `rh3`, `rh_c1`, `rh_c2` | `ram_tier_is_banded_and_never_falls_as_ram_rises`; **F1 + R1** |
| `orca-flow-control` | `t1`–`t4`, `kt1`–`kt3`, `c1`, `c2`, `kt_c1`, `kt_c2` | Exhaustive trace tests in `lib.rs` / `keep_tail.rs`; **R1** |
| `orca-crash-recovery` | `gf1`–`gf3`, `rr1`–`rr3`, `gf_c1`, `gf_c2`, `rr_c1`, `rr_c2` | Exhaustive crash-trace tests in `gpu_fallback.rs` / `renderer_recovery.rs` |
| `orca-session-gc` | `ex1`–`ex3`, `ev1`, `ev2`, `ev_step_monotone`, `ex_c1`, `ev_c1` | Exhaustive store-enumeration tests in `lib.rs`; **R1** |

## Priority

1. **S1** — unblocks the largest measured residue and stops spurious refutations
   from making `strict` unusable. Everything else is smaller with it done.
2. **N1** — cheapest item here, and it protects against the exact class of
   silent non-result that let the retired bundles read green.
3. **C1** — turns comment-assumptions into declared, auditable ones.
4. **I1**, **R1** — needed to retire the "covered meanwhile by a test" column.
5. **F1** — narrowest blast radius; the differential corpus is a good answer for
   equivalence regardless.
