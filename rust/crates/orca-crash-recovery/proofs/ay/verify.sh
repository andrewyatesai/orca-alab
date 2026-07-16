#!/usr/bin/env bash
# Re-checkable certificate for orca-crash-recovery (moonshot E1's "machine-checked
# safety certificate on the emitted code"). Discharged by `ay` (SAT/SMT/CHC solver)
# on hand-encoded SMT-LIB2 — no trust-mc needed.
# Run: `bash verify.sh`. Exits 0 iff every obligation gets its expected verdict.
#
# RENDERER-RECOVERY rate limiter — negation asserted, UNSAT == proved:
#   rr1_never_exceeds_max    inductive safety: post-count <= max (=> at most max per window)
#   rr2_no_admit_at_cap      c >= max => rejected AND count unchanged
#   rr3_reset_reopens        c = 0 (post-reset), max >= 1 => allowed (no permanent lockout)
#   rr_c1_reject_reachable_sat (control) the open-breaker/reject branch is reachable
#   rr_c2_admit_reachable_sat  (control) the admit branch is reachable and grows the count
#
# GPU-FALLBACK one-shot latch — negation asserted, UNSAT == proved:
#   gf1_engages_at_most_once      already engaged => no-op (relaunch at most once)
#   gf2_window_gate               crash outside [0, window] => no-op
#   gf3_no_engage_below_threshold engaged => post-count >= threshold
#   gf_c1_engage_reachable_sat        (control) the latch can actually trip
#   gf_c2_upper_boundary_inclusive_sat(control) m = window counts (inclusive edge, off-by-one catch)
#
# Both cores use integer-only decisions, so the in-window count is modelled as a
# free integer and the whole thing lives in QF_LIA. Together with the differential
# parity corpora (renderer-recovery / gpu-fallback, each a trace replayed by BOTH
# the Rust core and the TS class) this is the full E1 pair.
set -u

AY=""
for c in "$HOME/.cargo/bin/ay" \
         "$HOME/trust/build/host/stage2/bin/ay" \
         "$(command -v ay 2>/dev/null || true)"; do
  if [ -n "$c" ] && [ -x "$c" ]; then AY="$c"; break; fi
done
if [ -z "$AY" ]; then
  echo "FAIL: no runnable ay found (looked in ~/.cargo/bin, trust sysroot, PATH)" >&2
  exit 1
fi

cd "$(dirname "$0")"
rc=0
check() {
  local file="$1" want="$2"
  local got
  got="$("$AY" "$file" 2>/dev/null | grep -Ex 'sat|unsat|unknown' | tail -1)"
  if [ "$got" = "$want" ]; then
    echo "ok   $file  => $got"
  else
    echo "FAIL $file  => got '$got', want '$want'"
    rc=1
  fi
}

for t in rr1_never_exceeds_max rr2_no_admit_at_cap rr3_reset_reopens \
         gf1_engages_at_most_once gf2_window_gate gf3_no_engage_below_threshold; do
  check "$t.smt2" unsat
done
for c in rr_c1_reject_reachable_sat rr_c2_admit_reachable_sat \
         gf_c1_engage_reachable_sat gf_c2_upper_boundary_inclusive_sat; do
  check "$c.smt2" sat
done

if [ "$rc" -eq 0 ]; then echo "ALL PROOFS DISCHARGED"; else echo "PROOF FAILURE"; fi
exit "$rc"
