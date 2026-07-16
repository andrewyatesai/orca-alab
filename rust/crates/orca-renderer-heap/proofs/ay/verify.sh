#!/usr/bin/env bash
# Re-checkable certificate for orca-renderer-heap (moonshot E1's "machine-checked
# safety certificate on the emitted code"). Discharged by `ay` (SAT/SMT/CHC solver)
# on hand-encoded SMT-LIB2. Run: `bash verify.sh`. Exits 0 iff every obligation gets
# its expected verdict.
#
# THEOREMS (negation asserted; UNSAT == proved for all inputs):
#   rh1_band_bound                 ceiling in [FLOOR=3072, CAP=4096] for every target t>=0
#   rh2_clamp_monotone             t1<=t2 => clamp(t1)<=clamp(t2) (ceiling monotone in RAM)
#   rh3_floor_redundant_under_gate t>=3072 (the gated invariant) => max(3072,t)=t (floor is dead)
# CONTROLS (prove-AND-catch; SAT == non-vacuous):
#   rh_c1_cap_active_sat           the CAP fires at t>=4096 (band tight at the top)
#   rh_c2_gate_bottom_sat          the band bottom t=3072 is reached at the gate (tight)
#
# The target t = floor(totalGiB*0.4)*1024 is abstracted as a free integer, so the
# clamp-band reasoning lives in QF_LIA (no floats in the solver). The float layer —
# that JS Number and Rust f64 compute the same t — is pinned separately by the
# differential parity corpus (parity-corpus.txt), run by BOTH the Rust core and the
# TS production sizing. Together that is the full E1 pair.
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

for t in rh1_band_bound rh2_clamp_monotone rh3_floor_redundant_under_gate; do
  check "$t.smt2" unsat
done
for c in rh_c1_cap_active_sat rh_c2_gate_bottom_sat; do
  check "$c.smt2" sat
done

if [ "$rc" -eq 0 ]; then echo "ALL PROOFS DISCHARGED"; else echo "PROOF FAILURE"; fi
exit "$rc"
