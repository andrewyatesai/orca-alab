#!/usr/bin/env bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
#
# Re-checkable certificate for initiative A1: the row_index() fast path returns an
# IN-BOUNDS physical row index (licenses get_unchecked at storage.rs:400/:417).
# Discharged by `ay` (the Trust SAT/SMT solver) on hand-encoded SMT-LIB2 — Trust,
# NOT kani. Run: `bash verify.sh`. Exits 0 iff every obligation gets its verdict.
#
# OBLIGATIONS:
#   row_index_in_bounds          unsat  (ring_head+base) % len < len  for all len!=0
#   row_index_no_overflow        unsat  ring_head+base does not wrap under len bound
#   row_index_nonvacuity_sat     sat    strict-interior indices are reachable
#   row_index_catches_false_tight sat   the false tighter bound idx<=len-2 is caught
set -u
AY=""
for c in \
  "$HOME/.cargo/bin/ay" \
  "$HOME/trust/build/host/stage2/bin/ay" \
  "$HOME/trust/build/aarch64-apple-darwin/stage3-tools-bin/aarch64-apple-darwin/ay" \
  "$HOME/trust/build/aarch64-apple-darwin/stage2-tools-bin/aarch64-apple-darwin/ay" ; do
  if "$c" --version >/dev/null 2>&1; then AY="$c"; break; fi
done
[ -n "$AY" ] || { echo "FATAL: no runnable ay found"; exit 2; }
echo "ay = $AY"
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
expect() { # <file> <sat|unsat>
  local f="$DIR/$1" want="$2" got
  got=$("$AY" solve "$f" -t:120000 2>/dev/null | grep -iE '^(sat|unsat|unknown)$' | head -1 | tr '[:upper:]' '[:lower:]')
  if [ "$got" = "$want" ]; then printf '  PASS  %-34s %s\n' "$1" "$got"; return 0
  else printf '  FAIL  %-34s got=%s want=%s\n' "$1" "${got:-<none>}" "$want"; return 1; fi
}
echo "A1 — row_index() in-bounds (ay):"
rc=0
expect row_index_in_bounds.smt2           unsat || rc=1
expect row_index_no_overflow.smt2         unsat || rc=1
expect row_index_nonvacuity_sat.smt2      sat   || rc=1
expect row_index_catches_false_tight.smt2 sat   || rc=1
if [ "$rc" = 0 ]; then echo "A1: ALL OBLIGATIONS DISCHARGED ✓"; else echo "A1: FAILED ✗"; fi
exit "$rc"
