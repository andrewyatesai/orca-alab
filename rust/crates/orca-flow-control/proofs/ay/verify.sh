#!/usr/bin/env bash
# Re-checkable certificate for orca-flow-control (P3 stage 3, moonshot E1's
# "machine-checked safety certificate on the emitted code"). Discharged by `ay`
# (SAT/SMT/CHC solver) on hand-encoded SMT-LIB2 — no trust-mc needed.
# Run: `bash verify.sh`. Exits 0 iff every obligation gets its expected verdict.
#
# PRODUCER CONTROLLER (ProducerFlowController) — negation asserted, UNSAT == proved:
#   t1_no_flap                    paused + pending in [LOW,HIGH] => no action (anti-flap)
#   t2_reassert_gated             paused + flooding + elapsed<REASSERT => no re-Pause
#   t3_no_spurious_resume         paused + pending>=LOW => not Resume (strict low edge)
#   t4_unpaused_pause_iff_over_high  unpaused: Pause  <=>  pending > HIGH (strict high edge)
#   c1_reassert_reachable_sat     (control) the reassert path is reachable (t1/t2 not vacuous)
#   c2_catches_off_by_one_sat     (control) a `> HIGH-1` off-by-one bound IS caught
#
# KEEP-TAIL SIZING (background_session_keep_tail_chars) — negation asserted, UNSAT == proved:
#   kt1_clamp_bound               keep_tail in [64K, 512K] for every divide result
#   kt2_drop_cap_bound            drop_cap = 2*keep_tail in [128K, 1M]
#   kt3_clamp_monotone            x1>=x2 => clamp(x1)>=clamp(x2) (the monotone-in-n leg)
#   kt_c1_floor_active_sat        (control) the 64K floor is reached — band tight below
#   kt_c2_cap_active_sat          (control) the 512K cap is reached — band tight above
#
# Together with the differential parity corpora (parity-corpus.txt and
# keep-tail-parity-corpus.txt, each run by BOTH the Rust core and the TS production
# code), this is the full E1 pair: the specs are proved correct here, the
# implementations are proved equivalent to them there.
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
  # `ay solve` prints sat/unsat on its own line; grab the last verdict token.
  local got
  got="$("$AY" "$file" 2>/dev/null | grep -Ex 'sat|unsat|unknown' | tail -1)"
  if [ "$got" = "$want" ]; then
    echo "ok   $file  => $got"
  else
    echo "FAIL $file  => got '$got', want '$want'"
    rc=1
  fi
}

for t in t1_no_flap t2_reassert_gated t3_no_spurious_resume t4_unpaused_pause_iff_over_high \
         kt1_clamp_bound kt2_drop_cap_bound kt3_clamp_monotone; do
  check "$t.smt2" unsat
done
for c in c1_reassert_reachable_sat c2_catches_off_by_one_sat \
         kt_c1_floor_active_sat kt_c2_cap_active_sat; do
  check "$c.smt2" sat
done

if [ "$rc" -eq 0 ]; then echo "ALL PROOFS DISCHARGED"; else echo "PROOF FAILURE"; fi
exit "$rc"
