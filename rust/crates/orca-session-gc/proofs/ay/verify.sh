#!/usr/bin/env bash
# Re-checkable certificate for orca-session-gc (moonshot E1's "machine-checked
# safety certificate on the emitted code"). Discharged by `ay` (SAT/SMT/CHC solver)
# on hand-encoded SMT-LIB2. Run: `bash verify.sh`. Exits 0 iff every obligation
# gets its expected verdict.
#
# AGE-EXPIRY decision (negation asserted; UNSAT == proved):
#   ex1_live_never_expires                       a live dir is never expired
#   ex2_toctou_floor_never_expires               age < minDirAge => never expired
#   ex3_unknown_liveness_unrestored_never_expires  liveness unknown + not-ended => never
# SIZE-CAP eviction (negation asserted; UNSAT == proved):
#   ev1_never_below_nonevictable                 remaining >= non-evictable bytes (live/recoverable safe)
#   ev2_reaches_budget_when_enough               enough evictable => remaining reaches budget
#   ev_step_monotone                             each eviction step never raises remaining
# CONTROLS (prove-AND-catch; SAT == non-vacuous):
#   ex_c1_ended_expiry_reachable_sat             an ended dir past retention DOES expire
#   ev_c1_eviction_reaches_budget_sat            eviction can bring the store under budget
#
# Flags (live/ended/liveness-unknown) are Bools and byte totals are free ints, so
# everything is QF_LIA. Together with the differential parity corpus
# (parity-corpus.txt, run by BOTH the Rust core and the TS planner) this is the full
# E1 pair; the fs scan/rmSync executor around the planner is unchanged and covered
# by the 12 history-retention integration tests.
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

for t in ex1_live_never_expires ex2_toctou_floor_never_expires \
         ex3_unknown_liveness_unrestored_never_expires \
         ev1_never_below_nonevictable ev2_reaches_budget_when_enough ev_step_monotone; do
  check "$t.smt2" unsat
done
for c in ex_c1_ended_expiry_reachable_sat ev_c1_eviction_reaches_budget_sat; do
  check "$c.smt2" sat
done

if [ "$rc" -eq 0 ]; then echo "ALL PROOFS DISCHARGED"; else echo "PROOF FAILURE"; fi
exit "$rc"
