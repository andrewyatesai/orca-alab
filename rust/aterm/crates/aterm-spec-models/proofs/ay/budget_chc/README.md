<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Andrew Yates -->

# A8 — scrollback evicting-push byte-budget invariant (discharged by `ay`)

Re-checkable certificate for initiative **A8**: the aterm scrollback evicting-push
path keeps its **budgeted (hot+warm) byte count bounded** — the OOM-impossibility
theorem. Hand-encoded SMT-LIB2 **CHC (constrained Horn clauses, `(set-logic HORN)`)**,
discharged directly by `ay` (the SAT/SMT/CHC solver). **No `trust-mc` needed.**

Run `bash verify.sh` (it locates `ay` even while the trust sysroot rebuilds). It
exits 0 iff every obligation gets its expected verdict.

**CHC polarity** (verified on this box): `sat` = an inductive invariant EXISTS = SAFE
(ay prints the synthesized invariant as an `ay-chc-cert`); `unsat` = the error state
is REACHABLE = UNSAFE (ay prints a counterexample trace).

## Faithful source semantics (verified against the live tree, 2026-06-20)

| Concept | Source |
|---|---|
| STATE `b` = `budgeted_bytes` = `hot.budgeted_bytes() + warm.budgeted_bytes()` (**cold tier excluded**) | `disk_backed.rs:425` (`sync_accounting`) |
| `n` = `memory_budget`, always `>= 1` (`budget.max(1)`) | `disk_backed.rs:405` (`set_memory_budget`) |
| PUSH: `budgeted_bytes += line.memory_used()` (adds `step`, `1 <= step <= k = K_max`) | `hot_tier.rs:45-50` (`push`) |
| `over_budget()  <=>  b > n` | `lib.rs:409-410` |
| EVICTION after each push: `while over_budget() && warm.block_count() > 0 { evict_warm_to_cold(); }` (moves warm→**cold/excluded** ⇒ `b` decreases; stops at `b<=n` whenever warm had blocks) | `disk_backed_tiers.rs:69` (`handle_memory_pressure`) |

## What is proved

The honest **inductive** property (the doc's bounded-increment shape
`x' = x + K` under guard `x <= N`  ⇒  `x <= N + K`): a push advances the budgeted
byte count by at most one max line (`+step`, `step <= k`) **only from an
at-or-under-budget STABLE state** (`b <= n`), and eviction then restores the stable
bound. Therefore the **observable peak** byte count obeys

> **`budgeted_bytes  <=  memory_budget + K_max`**   (overshoot by at most one max push)

`ay` synthesizes exactly this as the inductive invariant on `budget_safe.smt2`:

```
(define-fun inv ((b Int) (n Int) (k Int)) Bool
  (and (>= b 0) (>= n 1) (>= k 1) (<= b (+ n k))))
```

| File | Verdict | Obligation |
|---|---|---|
| `budget_safe.smt2` | **sat** | faithful evicting-push system; ay synthesizes `b <= n+k` (the OOM-impossibility bound holds for **all** budgets `n>=1` and **all** max line sizes `k>=1`) |
| `budget_buggy.smt2` | **unsat** | eviction **and** the stability guard removed (a real failure mode: a scrollback that never reclaims) ⇒ unbounded growth; ay returns a counterexample (`n=1, k=2`: `b = 0→2→4 > n+k=3`) |
| `budget_catches_false_unconditional.smt2` | **unsat** | the checker **catches the FALSE unconditional bound** `b <= n`: a single push from `b=n` transiently overshoots, so the over-strong claim is reachable-false |
| `budget_bound_is_tight.smt2` | **unsat** | `n+k` is the **least** upper bound: the one-byte-tighter `b <= n+k-1` is reachable-false, so the proved bound is exact, not loose |

**Prove-and-catch non-vacuity** (the `assert_proves_and_catches` discipline):
`budget_safe` (sat: the honest bound holds) is paired with three `unsat` controls —
`budget_buggy` (the safeguard is load-bearing), `budget_catches_false_unconditional`
(the model genuinely admits the transient overshoot we tolerate), and
`budget_bound_is_tight` (the bound is exact). The SAFE proof is not passing because
the bound is trivially loose or the model is degenerate.

## Honest scope caveat — what this does NOT prove

- **NOT** the unconditional `budgeted_bytes <= memory_budget`. That claim is **false**:
  `handle_memory_pressure` runs *after* a push, and `hot_tier::push` does
  `budgeted_bytes += step` unconditionally, so a single push transiently overshoots
  by up to `K_max` bytes before eviction. `budget_catches_false_unconditional.smt2`
  is the on-the-record witness that we did not slip this false claim into the bundle.
- **Modeled eviction case = "warm had blocks to give."** The real loop also terminates
  when `warm.block_count() == 0` while still over budget — i.e. the un-evictable **hot**
  tier alone can exceed `n` (the doc's HONEST caveat). This bundle proves the bound for
  the bounded-increment regime where eviction restores the stable state; it does **not**
  bound the pathological all-hot configuration (that is a configuration constraint on the
  hot-tier limit, not a property of the evicting-push transition).
- Scoped **strictly to the byte budget `B`** (`budgeted_bytes`). It says nothing about the
  line-count budget `T`, disk/cold-tier size, compaction, or per-line memory accounting
  fidelity (`line.memory_used()`); only that the hot+warm byte aggregate cannot grow without
  limit along the evicting-push path.

## Engine-frontier note

This obligation lives in `ay`'s comfort zone: linear integer arithmetic CHC with a
single bounded increment. No reformulation was needed — the faithful encoding
(guarded `+step` push, eviction-restores-stable consecution, `b > n+k` query)
discharges directly, with `ay` synthesizing the textbook bounded-increment invariant
`b <= n+k`. Keeping `n` and `k` as frame-constant parameters (not fixed numerals)
makes the certificate hold for every budget/line-size pair rather than one instance,
and stays within the linear-arithmetic fragment ay solves in well under a second.
