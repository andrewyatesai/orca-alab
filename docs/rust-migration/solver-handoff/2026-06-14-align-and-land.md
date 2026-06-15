# 2026-06-14 — aligned with owner's latest; landed var×const + Unsize

## Outcome

- **var×const (Range::contains lever) + array→slice Unsize fix** — validated on the
  owner's latest trust (build #63), sound (all 4 soundness gates FAIL — no
  false-PROVE), and confirmed **non-redundant** (those cases all still fail on plain
  `origin/main`). Committed `db8302ad7e` on branch `ay-contains-lever-unsize-fix`
  (off `origin/main`), pushed to the private origin.
- **LRA non-termination — SUPERSEDED by the owner's parallel fix.** My ~6-build budget
  effort was correct in diagnosis (a stack `sample` pinned it to `compute_implied_bounds`
  doing unbounded GMP-bignum arithmetic — infinite-precision rational descent; the
  wall-clock deadline can't stop it because the solver never returns to poll it), but the
  owner had already fixed the *same* thing in ay `origin/main`: `de7053e` "throttle **Zeno**
  implied-bound cascades", `50e37ad` "restrain BCP-time implied-bounds cascade", `4922225`
  "sat-side livelock fix". My budget patch (`/tmp/my-ay-lra.patch`) was dropped as redundant.

## Alignment (per "align with main and remote main")

The owner had pushed heavily in parallel: **trust `origin/main` +21 commits** (13 touching
the exact vcgen/mir-extract files — guarded slice indexing, chunks_exact, multiply-in-contracts,
min/max bounds, nonlinear-relaxation retry) and **ay `origin/main` +139 commits** (incl. the
Zeno fix and the now-committed `AY_DIRECT_SOLVE_TIMEOUT_MS`). Actions:
- trust → fast-forwarded to `origin/main`; my var×const/Unsize stash **auto-merged cleanly**
  (no conflicts) onto their latest.
- ay pin → advanced `7346834 → origin/main` (Zeno) on the branch (`x.py check` confirmed
  they compile together). NOTE: `x.py` re-syncs the submodule to the *committed* gitlink, so
  the pin bump must be **committed** for the build to use the Zeno ay (builds #62/#63 used the
  pre-Zeno pin; build #64 commits the bump).

## KEY BLOCKER: the Zeno fix can't be pinned into trust yet — ay↔trust API drift

The survey/gap stays blocked, and the reason is now pinned down. The Zeno LRA fix lives only in
ay `origin/main` (139 commits ahead of the trust pin). Bumping trust→ay to that commit FAILS to
build: `E0599: no method 'get_timeout' / 'get_value' / 'get_model' / 'get_interrupt_handle' on
ApiSolver` (20 errors) — ay `origin/main` changed the `ApiSolver` API and the owner's trust
`origin/main` has NOT been migrated to it. **That is exactly why the owner pins the old ay
(`7346834`)**: their trust isn't yet compatible with the newer ay. So pulling the Zeno fix into a
buildable trust requires the owner's trust↔ay API migration (their core integration work) — NOT a
one-line pin bump. The pin-bump commit was dropped; trust stays on `7346834`.

Consequence: the orca-core survey can't converge from this tree (old ay pin ⇒ the original
implied-bound non-termination; Zeno fix unreachable without the API migration). The **gap metric**
stays blocked on that owner-side migration. My var×const/Unsize do NOT depend on it — validated by
isolated probes on build #63 and landed on branch `ay-contains-lever-unsize-fix`. (`x.py` re-syncs
the submodule to the *committed* gitlink, so an incremental `x.py check` can falsely pass against a
stale-cached ay-bridge crate — only a full recompile exposes the API drift.) Also cleared a 158-min
zombie `trustc` that had been starving CPU.
