#!/usr/bin/env bash
# Re-checkable certificate for orca-provider-backoff (moonshot E1's "machine-checked
# safety certificate on the emitted code"). Discharged by `ay` (SAT/SMT/CHC solver)
# on hand-encoded SMT-LIB2 — no trust-mc needed.
# Run: `bash verify.sh`. Exits 0 iff every obligation gets its expected verdict.
#
# THEOREMS (negation asserted; UNSAT == proved for all inputs):
#   bo1_throttle_bound   throttle in [BASE=30s, MAX=15min] for every multiplier p>=1
#   bo2_monotone         p1<=p2 => throttle(p1)<=throttle(p2) (non-decreasing in streak)
#   bo3_saturates        p>=ceil(MAX/BASE)=30 => throttle = MAX (pins to the ceiling)
# CONTROLS (the prove-AND-catch discipline; SAT == non-vacuous / caught):
#   bo_c1_unsaturated_reachable_sat  the doubling takes intermediate values (not vacuous)
#   bo_c2_floor_tight_sat            the BASE floor is reached (band tight; strict `>BASE` is false)
#
# The multiplier p abstracts 2^max(0, streak-1) as a free integer p>=1; since the
# real streak values are a subset {1,2,4,...}, proving over ALL p>=1 is strictly
# stronger. Overflow-safety of the FINITE-WIDTH Rust impl (the `1u64 << exp` never
# panics for any u32 streak) is a separate obligation, discharged by the crate's
# stays_in_the_backoff_band / saturates_and_stays_saturated tests (called at
# u32::MAX). Together with the differential parity corpus (parity-corpus.txt, run by
# BOTH the Rust core and the TS production sizing) this is the full E1 pair.
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

for t in bo1_throttle_bound bo2_monotone bo3_saturates; do
  check "$t.smt2" unsat
done
for c in bo_c1_unsaturated_reachable_sat bo_c2_floor_tight_sat; do
  check "$c.smt2" sat
done

if [ "$rc" -eq 0 ]; then echo "ALL PROOFS DISCHARGED"; else echo "PROOF FAILURE"; fi
exit "$rc"
