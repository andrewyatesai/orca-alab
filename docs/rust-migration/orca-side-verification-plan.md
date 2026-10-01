# Getting Orca's obligations to green — the Orca-side plan

Companion to `trust-goal-real-obligations.md`, which is the directive for the
**Trust** repo. This one covers what changes in **Orca**, and it starts by
refusing the metric it was asked for.

## The target is not "100% proved, 0% failed, 0% unknown"

That is the shape of the claim `trust-goal-real-obligations.md` already records
as a failure: "orca-core: 244 proved" was 244 vacuous
`trust_mc_default_function` placeholders, while every real obligation was
`unknown`. A percentage is satisfiable by deleting obligations, by weakening
code until nothing is checkable, and by counting admissions as proofs. Three of
those routes are open in Orca today and one of them is *cheap* — see "Do not
reach for this" below.

The target is instead:

> Every real obligation on an authority-bearing crate is **proved
> non-vacuously**, or **explicitly assumed with a named reason**. No obligation
> anywhere carries a verified counterexample. The set of assumed obligations
> only shrinks.

Note the second clause is deliberately not "`failed` is zero". `FAILED` in this
report does not mean what it sounds like, and the next two sections are about
why that distinction is the whole plan.

## First, a warning about counting

Before quoting any of these numbers, know that **the report does not reconcile
with itself.** One advisory survey of orca-core (2026-08-08) yields four
different answers to "how many failed":

| Aggregation | Failed |
| --- | --- |
| Sum of `note: Trust verification: … N failed` over 400 report lines | 69 |
| Sum of `Level 0 summary: N failed` over the same 400 | 81 |
| Explicit `[kind] FAILED` rows | 46 |
| `[kind] FAILED (…); counterexample: …` rows | 34 |

These count different things (Level 0 vs all levels, per-function vs
per-obligation, with and without a structured counterexample field). None is
wrong. But "orca-core has N failed obligations" is not a sentence anyone can say
without naming the aggregation, and any plan phrased as a percentage inherits
that ambiguity four times over.

**So state the query with the number.** Everything below does.

## What the numbers actually decompose into

Measured on `orca-policy` (2026-08-08), a new crate written specifically to be
easy to verify — zero dependencies, `forbid(unsafe_code)`, no IO, no clock. It
is the best case, which is what makes it a useful ruler.

Three source changes — derives behind `cfg(test)`, `(high << 4) | low` in place
of `high * 16 + low`, and a constant allocation bound — took it from **22
unverified obligations across 17 functions with 3 refutations** to this:

| Function | Obligations | Proved | Runtime-checked | Failed |
| --- | --- | --- | --- | --- |
| `hex_value` | 5 | **5** | 0 | 0 |
| `decode_once` | 2 | **1** | 1 | 0 |
| `decide_play_path_lexical` | 1 | 0 | 1 | 0 |
| `is_allowed_play_host` | 1 | 0 | 1 | 0 |
| `is_windows_device_segment` (+closure) | 2 | 0 | 2 | 0 |
| `has_allowed_extension` (+closure) | 2 | 0 | 2 | 0 |
| `decide_fleet_grant` (2 closures) | 2 | 0 | 2 | 0 |
| **Total** | **15** | **6** | **9** | **0** |

Note what the arithmetic rewrite actually bought: it did not merely delete two
refutations, it made them **provable** — `hex_value` discharges 5 of 5 (3
kernel-certified by the clean CIC kernel) and `decode_once` now proves 1 of 2.

**Count obligations from the per-function report lines, not from `[kind]`
markers.** Proved obligations emit no `[kind]` marker, so grepping markers alone
reports 9 here and silently drops the 6 that succeeded. That mistake makes
progress invisible.

All nine survivors are `[assert] runtime-checked` raised by **absent callees** —
`String`, `Vec`, `str`, iterator adapters. No source change reaches them; that
is the Trust-Std campaign. `hex_value`, the one function in the crate that
touches only `u8`, proves completely.

That gives the boundary in one sentence, and it is confirmed by the tree: the
only crate holding `certify` is `orca-provider-backoff`, which is a single
`fn(u32) -> u64`.

> **Today, Trust proves scalar functions. Anything that touches the heap or the
> string/iterator APIs is blocked on Trust-Std, no matter how it is written.**

## Before believing any "proved": two checks, every time

`trust-goal-real-obligations.md` exists because 244 proofs turned out to be
placeholders. So a `proved` count is a claim to be tested, not a result. Both
checks are cheap and both must pass.

**Negative — is it the known-vacuous placeholder?**

```sh
grep -c "trust_mc_default_function" survey.txt   # must be 0
grep -cE "bool_literal|vacuous" survey.txt       # must be 0
```

**Positive — does the proof fail when the code is wrong?** Plant a real defect
in a proved function and re-run. On `hex_value`, replacing `byte - b'0'` with
`byte - b'a'` (which underflows for `'0'`) turns *5 proved* into *4 proved + 1
`[overflow:sub] FAILED, verified_counterexample = true`*.

The negative check alone is not enough. A proof nobody has watched fail is
indistinguishable from a proof that cannot fail — which is precisely how the 244
survived review.

## The single most important distinction: is there a counterexample?

`FAILED` does not mean "Trust found a bug." It means the obligation was not
established, and the report tells you which by whether the `counterexample:`
field is populated:

```sh
grep -c "FAILED (.*); counterexample: verified_counterexample = true"  # real refutations
grep -cE "FAILED \(.*\); counterexample: *$"                           # unestablished
```

Run against orca-core's survey, the first query returns **0**. Every structured
failure it has is `[hardened_unsafe_operation]` from `ay-in-process` with an
**empty** counterexample — 32 of the 34 rows that carry the field. They cluster
in `cross_platform_path.rs` (14), `quick_open_filter.rs` (12) and
`setup_runner_command.rs` (7): ordinary string and slice code with no `unsafe`
in it anywhere, so what is unestablished is std's hardening, not Orca's logic.

The same query against `orca-policy` as first written returned **2**
(`[overflow:mul]`, `[overflow:add]`, both `verified_counterexample = true`).
Those were real, and both are now fixed.

That is the whole triage rule:

> **A populated counterexample is a work item. An empty one is a coverage gap.**
> Treating them alike is how a 34-row list becomes an unfixable backlog and
> everyone stops reading it.

The only orca-core rows that carry a real counterexample are 2
`[unbounded_allocation]` in `quick_open_filter.rs` — and see step 3, because the
orca-policy fix does **not** transfer to them unchanged.

So the buckets, and who can close them:

1. **Populated counterexamples — Orca closes these. Always.**
   orca-core: 2, both `[unbounded_allocation]`. orca-policy: was 3, now 0.
   This is a small, finite, actionable list — which is the point of separating
   it from the 425 rows that are not.
2. **Derived boilerplate — Orca closes these, and it is nearly free.**
   Measured on orca-core's 2026-08-08 survey, over its 400 functions carrying
   542 unproved obligations:

   | | Functions | Unproved obligations | Share |
   | --- | --- | --- | --- |
   | derive-generated | 146 | 146 | **26.9%** |
   | first-party | 254 | 396 | 73.1% |

   Split by trait: `Debug::fmt` 72, `Clone::clone` 33, `PartialEq` 28,
   `Default` 13. Exactly one unproved obligation each — they are uniform
   boilerplate, which is why moving them behind `cfg(test)` is a single
   mechanical edit rather than 146 decisions.

   Note this is **26.9%, not the 93% ("280 of 302") that was in circulation**.
   That figure does not appear anywhere in this survey. The available win is
   about a quarter of the backlog — worth taking, and worth not overselling.
3. **Empty-counterexample failures and `[assert] runtime-checked` — only Trust
   closes these.** 393 `[assert] runtime-checked` rows in orca-core, plus the 32
   hardened rows. This is the absent-callee std gap, ~69% of obligations
   workspace-wide per the retired `rust/.cargo/config.toml` notes (f2a257117). Any Orca-side plan that
   promises to zero this bucket is lying.

## The work, in the order it pays

**1. Zero the rows with a populated counterexample.**

orca-policy is already there (3 → 0). orca-core has 2, both in step 3. The
technique for arithmetic, from the two that were real:

Prefer a form that raises **no** obligation over one that raises a provable
obligation. `(high << 4) | low` beats `high * 16 + low`: identical for nibbles,
but `<<` only constrains the shift amount (a constant) and `|` cannot overflow,
whereas `*`/`+` oblige the verifier to know a range it cannot see across an
absent callee. Where a value is genuinely unbounded, use `checked_*`/
`saturating_*` and handle the case. Where an assertion is load-bearing, it wants
a real precondition (`#[trust::requires]`), not a rewrite.

**2. Move `derive(Debug, Clone)` behind `cfg(test)` on heap-bearing types —
per type, with the workspace build as the check.**

One caveat that will bite whoever does this in bulk: **`cfg(test)` is set only
while compiling that crate's own test target.** A dependent crate never sees the
impl — not in its source, and not in its tests either. So this is safe for a leaf
crate like `orca-policy` (verified: `orca-dispatch` builds against it), but on
`orca-core` it has to be applied per type and checked, because any dependent that
`{:?}`-prints or clones the type stops compiling. Where a dependent does need it,
the tool is a cargo feature, not `cfg(test)`.

Tested rather than assumed: gating `BaseRefSearchResult`'s `Debug` this way
builds `orca-core`/`orca-git`/`orca-dispatch` with 0 errors and leaves
`orca-git`'s tests and orca-core's own 294 green. That is one type of 146 — it
establishes the edit is viable, not that it is blanket-safe.

Eight of `orca-policy`'s original 22 obligations — 36% — were generated code
nobody reads. It costs nothing: the derives stay in test builds, which is the
only place they were used. Measure orca-core's derive share before committing to
a number for it; this is a cheap edit with a large measured effect on one crate,
which is a reason to try it, not a reason to quote a figure from elsewhere.

**3. Bound allocations by a constant — but only where the cap is semantically
free.**

`[unbounded_allocation]` was a *real* finding in `decode_once`: it allocated in
proportion to an attacker-controlled request path. Two things generalize:

- Adding `if path.len() > CAP { return None }` did **not** discharge it.
  `len()` is an absent callee, so a length-derived capacity stays
  uninterpretable no matter what guards it. `Vec::with_capacity(CAP)` — a
  literal constant — discharges it immediately. Guard the input *and* allocate
  the constant; the guard is for the semantics, the constant is for the proof.
- **It does not always transfer.** orca-core's 2 real rows are
  `vec![".."; f.len() - common]` in `posix_relative`/`win32_relative`
  (`quick_open_filter.rs`), which port Node's `path.relative`. Capping the
  segment count there would change ported behavior and break parity with the TS
  twin. A 4 KB cap on an HTTP request path costs nothing real; a cap on path
  depth is a semantic change. Where the cap is not free, the honest outcome is a
  **precondition or a named assumption**, not a silent behavior change to make a
  number move.

**4. Push authority decisions down to scalar cores where it is honest to.**

This is the only lever that converts `unknown` into `proved` today, and it is
narrow. It applies where a decision can be expressed over integers and enums
rather than strings. It does **not** apply to path containment, which is
irreducibly string-shaped — and pretending otherwise by hand-rolling byte
comparison to dodge `str` would be optimizing for the metric, not for safety.

**5. Report by kind, per crate, with the query that produced it.**

`refuted-with-counterexample` / `unestablished` / `proved` — three numbers,
always together, each with the `grep` that yields it. A single "proved" figure is
what made 244 vacuous placeholders look like progress, and as the table at the
top shows, a single "failed" figure is no better defined.

## Do not reach for this

The cheapest way to "100% proved, 0% unknown" is to delete obligations: drop the
`assert!`, index without bounds, stop deriving anything, replace `String` with
fixed buffers until the verifier has nothing to say. Every one of those raises
the percentage and lowers the safety. `trust-goal-real-obligations.md` puts it
directly — "green checkmarks for unproven or vacuous obligations are worse than
no verifier."

The corresponding discipline on the Orca side: **an obligation may only leave
the list by being proved or by being named as an assumption.** Never by the code
becoming less checkable.

## Not taken: retiring the `proofs/ay` bundles (the 2026-08-08 SMT purge)

A pair written on 2026-08-08 and never pushed (`ac5f15358` + `94b532c03`, kept on
branch `salvage/local-main-orca-alab-20260928`) deleted all 92 bundle files under
8 crates' `proofs/ay`, made the gauntlet certificates axis FAIL on any
`.smt2`/`.alethe`, and moved the safety axis to trustc's report for
`orca-stream-split`. Its additive half, the class-exhaustive property tests with
non-vacuity counters beside each bundle, landed as `48f96aaa8`. The retirement
was re-assessed on 2026-09-29 against `f51eb6497`, re-measured on 2026-09-30
against `3c9500365`, and is not taken:

- **It would drop obligations without proving them.** The pair's own stage-1
  measurement credits the compiler's verifier only with machine-integer
  (Level 0) obligations in `orca-provider-backoff` (1/1) and
  `orca-stream-split` (2/2), and its deletion inventory routes even those two
  crates' bundles to property tests (`bo2_monotone` is relational, its gap R1).
  So in all eight crates a property test would replace a discharged theorem,
  which the rule above forbids. On `3c9500365` all 14 `verify.sh` exit 0 with
  atpkg ay 0.13.0: 64 checks over 9 crates, 39 `unsat` theorems and 25 `sat`
  non-vacuity controls. 36 of those theorems are in the eight crates (the other
  3 are `orca-policy`'s, added after the purge), 29 of them in the six crates
  it credits the compiler with nothing in. None of the eight crates states a
  bundle's property as a `trust::ensures` or `trust::requires`. `orca-git`'s
  four contracts are in `git_history*.rs` and `source_control_ai.rs`, not in
  `status_stream.rs` or `orca-core`'s `git_cquoted_path.rs`, which its bundles
  model. So no bundle meets the retirement rule below today.
- **The overflow blind spot that motivated it is closed without it.** The
  bundles model QF_LIA over unbounded `Int`, so they cannot state machine
  overflow. `d933238fa` saturated the five reachable i64/u64 sites, and
  `targo trust check` reports machine-integer (Level 0) obligations on the real
  code. `pnpm verify:rust` runs it; its default crate set leaves out
  `orca-session-gc` and `orca-crash-recovery`, so name them or pass `--all`.
- **Main went the other way.** `10eaed1e9` added `orca-policy`'s
  `grant_authority` bundle after the purge was written. On 2026-09-29,
  `1ecb5abf2` made every bundle fail closed on a bad `$AY`.
- **Its gauntlet code is stale.** The safety axis runs `$TRUSTC` or a Trust
  checkout's `build/host/stage{1,2}` trustc with `-Ztrust-*` flags. Main
  resolves Trust from PATH through targo and passes verifier bounds in
  `TRUSTFLAGS` (`tools/trust-survey/survey-orca-verify.sh`). Its parity fix
  (stable cargo re-resolving to the `trust` channel) is superseded by main's
  `targo --unverified` run over every corpus crate. A replay onto `3c9500365`
  conflicts in 25 paths (22 for `ac5f15358`, 3 for `94b532c03`, by
  `git merge-tree`): the bundle drivers main has edited since, the Rust files
  whose tests already landed, the rate-limit backoff TS and the gauntlet
  files. Its capability-gap list
  (`docs/trust/capability-gaps-from-the-smt-purge.md` in `ac5f15358`) asks
  Trust for seven things: container length tied to its buffer (S1), callee
  postconditions (C1), struct invariants (I1), relational properties (R1),
  floating point (F1), vacuity and reachability reports (N1), and a `tippy`
  lint against hand-coded solver logic (T1). It was measured on the
  2026-08-08 stage 1. Re-measure it before quoting it.

Retire bundles one crate at a time, never all at once. A bundle may go only when
`targo trust check` at `certify` proves the same property on the real code, as a
measurement rather than an assumption. Until then the bundle and its property
test stay side by side.

## What is done

- `orca-policy` exists: `decide_play_path_lexical` and `decide_fleet_grant`,
  the two authority decisions from the modes work, ported from
  `src/main/story-world/play-path-guard.ts` and `src/shared/fleet-grant.ts`.
- Its refutation count is zero, down from three, all closed by steps 1 and 3 —
  and 6 of its 15 obligations now PROVE, up from none.
- Two parity mechanisms, because one was not enough:
  - `parity-corpus.txt`, run by both the Rust crate and
    `src/main/story-world/play-path-parity.test.ts`. It earned its keep
    immediately, catching a decode-position divergence that made
    `/%2e%2e/secrets.js` traversal on one side and an inert literal on the other.
  - `tools/parity` — the differential harness, which is strictly stronger
    because it diffs the port against the **live** TypeScript rather than
    against a transcribed golden. 27 vectors covering every denial reason on
    both decisions.

  A port is not finished when the Rust builds and the corpus is green. It is
  finished when something **calls** the dispatch arm. Mine had no caller and no
  test until the audit; it was shipped dead.
- The E1 gate runs that corpus. It did not before — two separate bugs
  (`-Z` rustflags under a stable toolchain, and parity discovery filtering the
  ay-certificate subset instead of all crates) made it report PASS without
  executing a single corpus. Both fixed, and the fix is verified by deletion:
  breaking one corpus row now takes the gate from PASS to FAIL.

## A corpus cannot catch what it cannot express

The cap divergence is the sharpest lesson so far, and it generalizes past this
crate.

Adding `MAX_REQUEST_PATH_BYTES` to the Rust core silently made the two sides
disagree: a 5000-byte path was denied by Rust and allowed by TS, which had no
cap at all. Every corpus row still passed — because no row was 5000 bytes long,
and none could reasonably be. The comment on the constant asserted the two sides
were held together. Nothing held them together.

**A shared corpus only covers properties expressible as a row.** For everything
else — limits, timeouts, sizes, capacities — declare the value in the corpus
HEADER and have each side assert its own constant against the declaration:

```
# max-request-path-bytes: 4096
```

Then drifting the value on either side reddens that side, and deleting the
enforcement reddens a separate behavioral case. Two guards, two distinct failure
modes, both watched failing.

The meta-lesson, which cost three defects this session: **a comment claiming an
invariant is not an invariant.** Every one of the three was a case where the
prose described a property the code did not implement. If a comment asserts two
things agree, there must be a test that fails when they do not.

## What is next

- The 2 populated-counterexample rows, now localized precisely:
  `orca_core::quick_open_filter::normalize_segments` (a `Vec` that grows one
  push per path segment) and `win32_relative::{closure#0}` (a `String` per
  segment). Both are `[unbounded_allocation]`, and both are unbounded *because
  the input is* — they port Node's `path.relative`, which has no depth limit
  either.

  The repo already has the right machinery for the bounded case:
  `orca-net/proofs/ay/oom_bound` discharges exactly this obligation class for
  the NDJSON splitter, and it is worth copying for its DISCIPLINE as much as its
  content — alongside the bound it ships `oom_catches_unguarded_sat.smt2` (the
  proof must reject an unguarded variant) and `oom_nonvacuity_sat.smt2` (the
  assumptions must be SAT), under the `assert_proves_and_catches` contract in
  `rust/PROOF_CARRYING_PERFORMANCE.md`. That is the institutional form of the
  two checks in "Before believing any proved" above.

  It does not transfer here, because there is no bound to prove. `quick_open_filter`
  therefore wants a **named assumption** — the allocation is proportional to path
  depth, the input is a workspace file listing rather than a peer on a socket,
  and the real bound is the filesystem's. Writing an `oom_bound`-shaped
  certificate for it would be asserting a limit the code does not enforce.
- Re-measure orca-core's derive share, then step 2 if it is as large as
  orca-policy's was.
- Take the 32 `[hardened_unsafe_operation]` rows to the Trust side as a single
  question: they are `forbid`-clean Orca code with no `unsafe` in it, so an
  empty counterexample there is a statement about std, and it should either
  prove or be declared.
- The remaining decision cores named in the modes work but still TS-only:
  `reconcileTaskClaim`, `route-key`/`store-key`/`pty-binding`,
  `collapseExceptionsByTask`.
- An ay certificate for `orca-policy`, so it joins the eight crates whose
  properties are discharged rather than merely reported. `oom_bound` is the
  template, and the E1 gate auto-discovers any crate with a
  `proofs/ay/verify.sh` — so the certificate is picked up the moment it lands.
  The obvious first goal is containment: `decide_play_path_lexical` never
  returns `NeedsRealpathCheck` for a path containing a `..` segment, with a
  catches-SAT twin that a guard-removed variant must violate.
- Two gate repairs of the same family, found while wiring the above, both of
  which made a green reading meaningless:
  - `pnpm parity` aborted at the Rust leg, so the TS↔Rust differential was not
    running for anyone. Both blocking divergences are now fixed and it exits 0
    at **1543/1543**. They needed opposite fixes, which is the point:
    `terminal-stream-protocol` was a real regression in the TS (the v1.4.165
    merge dropped `SetOutputPaused = 16`, so a host silently discards a client's
    pause frame and keeps flooding it), while `feature-tips` was merely a stale
    port and TS was already right. Parity makes the port match what SHIPS; it
    does not split the difference.
  - `cargo test -p orca-config` could not build — an `include_str!` still
    pointed at `tests/tools/parity/vectors/`, which no longer exists. 123 tests
    had not been running.
  - The E1 certificates gate had two bugs that made it PASS without executing a
    single corpus (`-Z` rustflags under a stable toolchain; parity discovery
    filtering the ay-certificate subset). Both fixed.

  The pattern is worth naming: **every gate in this area was green for a reason
  unrelated to the property it claims to check.** Before trusting any of them,
  break something on purpose and confirm the gate notices.
