#!/usr/bin/env bash
# Re-checkable certificate for orca-stream-split (moonshot E1's "machine-checked
# safety certificate on the emitted code"). Discharged by `ay` (SAT/SMT/CHC solver)
# on hand-encoded SMT-LIB2. Run: `bash verify.sh`. Exits 0 iff every obligation
# gets its expected verdict.
#
# THEOREMS (negation asserted; UNSAT == proved for all inputs):
#   cs1_never_splits_target_pair  clamp never returns `end` when (end-1,end) is a pair
#   cs2_in_range                  start<end<len => start <= result <= end
#   ns1_progress                  start<len => next_safe_split_index > start (no stall)
#   ns2_skips_full_pair           a pair at start => result = start+2 (kept together)
# CONTROLS (prove-AND-catch; SAT == non-vacuous):
#   cs_c1_clamp_active_sat        the surrogate clamp actually fires (returns end-1)
#   ns_c1_skip_active_sat         the whole-pair skip actually fires (returns start+2)
#
# UTF-16 code units are modelled as free integers in [0,65535] with the surrogate
# ranges as linear bounds (high 55296..56319, low 56320..57343), so the whole thing
# is QF_LIA. Together with the differential parity corpus (parity-corpus.txt, run by
# BOTH the Rust core and the TS clamp/next) this is the full E1 pair.
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

for t in cs1_never_splits_target_pair cs2_in_range ns1_progress ns2_skips_full_pair; do
  check "$t.smt2" unsat
done
for c in cs_c1_clamp_active_sat ns_c1_skip_active_sat; do
  check "$c.smt2" sat
done

if [ "$rc" -eq 0 ]; then echo "ALL PROOFS DISCHARGED"; else echo "PROOF FAILURE"; fi
exit "$rc"
