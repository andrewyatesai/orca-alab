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
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Solver ladder + the opt-in AY_SOLVER=z3 portability mode live in ../resolve-solver.sh.
. "$DIR/../resolve-solver.sh"
if [ -z "$SOLVER_BIN" ]; then
  if [ "$SOLVER_KIND" = "z3" ]; then
    echo "FAIL: AY_SOLVER=z3 requested but no runnable z3"
    exit 1
  fi
  echo "SKIP: no runnable ay found (line_scan_bounds not checked)"
  exit 0
fi
solver_banner
expect() { # <file> <sat|unsat>
  local f="$DIR/$1" want="$2" got
  got=$(solve_verdict "$f")
  if [ "$got" = "$want" ]; then printf '  PASS  %-38s %s\n' "$1" "$got"; return 0
  else printf '  FAIL  %-38s got=%s want=%s\n' "$1" "${got:-<none>}" "$want"; return 1; fi
}
echo "line_scan_bounds — single-scan line-split index arithmetic ($SOLVER_KIND):"
rc=0
expect line_scan_in_bounds.smt2               unsat || rc=1
expect line_scan_nonvacuity_sat.smt2          sat   || rc=1
expect line_scan_catches_false_strip_sat.smt2 sat   || rc=1
if [ "$rc" = 0 ]; then echo "line_scan_bounds: ALL OBLIGATIONS DISCHARGED ✓"; else echo "line_scan_bounds: FAILED ✗"; fi
exit "$rc"
