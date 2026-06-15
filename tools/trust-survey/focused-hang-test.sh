#!/usr/bin/env bash
# Focused test: does the entry-guard LRA work budget make a known-hanging orca-core
# function TERMINATE? Targets ONE function via TRUST_VERIFY_FUNCTIONS, sets a LOW
# AY_IMPLIED_BUDGET so the budget trips before the GMP-bignum cascade explodes, and
# DISABLES the AY direct-solve timeout (SURVEY_NO_AY_TIMEOUT) so termination is proven
# by the budget alone — not masked by a wall-clock kill. A perl alarm is the only
# process-level backstop; exit 142 means it STILL hangs (budget failed).
#
# Usage: focused-hang-test.sh [function-substr] [budget] [alarm-seconds]
set -uo pipefail

FN="${1:-build_agent_notification_id}"
BUDGET="${2:-20000}"
ALARM_S="${3:-180}"

TRUST="${TRUST_HOME:-$HOME/trust}"
export WS="${ORC_RUST:-$HOME/orc/rust}"

TCARGO=""
for cand in \
  "$TRUST/build/aarch64-apple-darwin/stage2/bin/tcargo-trust" \
  "$TRUST/build/aarch64-apple-darwin/stage3-tools-bin/aarch64-apple-darwin/tcargo-trust" \
  "$TRUST/build/host/stage2/bin/tcargo-trust" \
  "$TRUST/build/aarch64-apple-darwin/stage0-sysroot/bin/tcargo-trust"; do
  [ -x "$cand" ] && { TCARGO="$cand"; break; }
done
[ -n "$TCARGO" ] || { echo "FATAL: no tcargo-trust under $TRUST/build" >&2; exit 2; }

OUT="/tmp/focused-hang-${FN}-b${BUDGET}.json"
LOG="/tmp/focused-hang-${FN}-b${BUDGET}.log"

export TRUST_VERIFY_SURVEY=1
export TRUST_VERIFY_POLICY="verify-example-corpus"
export TRUST_VERIFY_FUNCTIONS="$FN"
export TRUST_TIMEOUT_MS="${TRUST_TIMEOUT_MS:-120000}"   # typed-CHC watchdog stays on
export TRUST_VERIFY_FN_BUDGET_MS="${TRUST_VERIFY_FN_BUDGET_MS:-300000}"
export AY_IMPLIED_BUDGET="$BUDGET"
unset AY_DIRECT_SOLVE_TIMEOUT_MS                          # prove termination by budget alone

echo "tcargo : $TCARGO"
echo "fn     : $FN"
echo "budget : $BUDGET   alarm: ${ALARM_S}s   (AY direct-solve timeout DISABLED)"
echo "start  : $(date '+%H:%M:%S')"

# Bust cache so trustc actually re-verifies the crate.
[ -f "$WS/crates/core/src/lib.rs" ] && touch "$WS/crates/core/src/lib.rs"
[ -f "$WS/crates/orca-core/src/lib.rs" ] && touch "$WS/crates/orca-core/src/lib.rs"

SECONDS_START=$(perl -e 'print time')
perl -e 'chdir $ENV{WS} or die; alarm shift; exec @ARGV' "$ALARM_S" \
  "$TCARGO" trust check -p orca-core --format json --allow-l0-gaps >"$OUT" 2>"$LOG"
RC=$?
SECONDS_END=$(perl -e 'print time')
ELAPSED=$((SECONDS_END - SECONDS_START))

echo "exit   : $RC   elapsed: ${ELAPSED}s   at $(date '+%H:%M:%S')"
if [ "$RC" = 142 ] || [ "$RC" = 14 ]; then
  echo "!! STILL HANGS — entry-guard budget did NOT terminate $FN (alarm fired at ${ALARM_S}s)"
  echo "   => non-termination is NOT the outer-loop re-entry; it's a single non-returning call or SAT-core."
else
  echo "++ TERMINATED in ${ELAPSED}s (exit $RC). Outcome for $FN:"
  grep -oE '"status"[^,]*|"reason"[^,]*' "$OUT" 2>/dev/null | sort | uniq -c | head
fi
echo "json: $OUT   log: $LOG"
