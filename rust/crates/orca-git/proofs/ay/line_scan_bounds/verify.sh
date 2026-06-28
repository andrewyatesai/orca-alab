#!/usr/bin/env bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
#
# Re-checkable certificate for the SINGLE-SCAN line-splitting index arithmetic in
# the orca-git status scanner (see rust/PROOF_CARRYING_PERFORMANCE.md): given
# start<=nl<len, end=ite(cr && nl>start, nl-1, nl) satisfies start<=end<=nl<len
# with no usize underflow, and next start'=nl+1<=len. Discharged by `ay` (the
# Trust SAT/SMT solver) — Trust, NOT kani.
# Run: `bash verify.sh`. Exits 0 iff every obligation gets its verdict (or ay is
# absent, in which case the bundle is SKIPPED, not failed).
#
# OBLIGATIONS:
#   line_scan_in_bounds              unsat  start<=end<=nl<len, no underflow, nl+1<=len
#   line_scan_nonvacuity_sat         sat    start==nl collapses end to start (guard needed)
#   line_scan_catches_false_strip_sat sat   end==nl reachable w/o CR (false "always strip" caught)
set -u
AY=""
for c in \
  "$HOME/.cargo/bin/ay" \
  "$HOME/trust/build/host/stage2/bin/ay" \
  "$HOME/trust/build/aarch64-apple-darwin/stage3-tools-bin/aarch64-apple-darwin/ay" \
  "$HOME/trust/build/aarch64-apple-darwin/stage2-tools-bin/aarch64-apple-darwin/ay" ; do
  if "$c" --version >/dev/null 2>&1; then AY="$c"; break; fi
done
[ -n "$AY" ] || { echo "SKIP: no runnable ay found (line_scan_bounds not checked)"; exit 0; }
echo "ay = $AY"
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
expect() { # <file> <sat|unsat>
  local f="$DIR/$1" want="$2" got
  got=$("$AY" solve "$f" -t:120000 2>/dev/null | grep -iE '^(sat|unsat|unknown)$' | head -1 | tr '[:upper:]' '[:lower:]')
  if [ "$got" = "$want" ]; then printf '  PASS  %-38s %s\n' "$1" "$got"; return 0
  else printf '  FAIL  %-38s got=%s want=%s\n' "$1" "${got:-<none>}" "$want"; return 1; fi
}
echo "line_scan_bounds — single-scan line-split index arithmetic (ay):"
rc=0
expect line_scan_in_bounds.smt2               unsat || rc=1
expect line_scan_nonvacuity_sat.smt2          sat   || rc=1
expect line_scan_catches_false_strip_sat.smt2 sat   || rc=1
if [ "$rc" = 0 ]; then echo "line_scan_bounds: ALL OBLIGATIONS DISCHARGED ✓"; else echo "line_scan_bounds: FAILED ✗"; fi
exit "$rc"
