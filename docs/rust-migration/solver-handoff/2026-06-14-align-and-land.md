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

## Open: the orca-core survey is chronically uncompletable

Even on the owner's plain baseline (build #62, pre-Zeno ay) the survey hit the whole-run cap
with empty JSON — an "uncovered engine path" beyond the per-obligation timeout. Build #64
(Zeno ay) is the first real test of whether the Zeno fix closes it. My var×const/Unsize do not
depend on the survey (validated by isolated probes), but the **gap metric** stays blocked until
the survey converges. Also cleared a 158-min zombie `trustc` that had been starving CPU.
