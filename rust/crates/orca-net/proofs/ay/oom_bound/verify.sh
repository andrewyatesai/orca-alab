#!/usr/bin/env bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
#
# Re-checkable certificate for the orca-net NDJSON splitter's OOM byte-budget
# invariant (see rust/PROOF_CARRYING_PERFORMANCE.md for the proof-boundary
# contract): the retained buffer never exceeds max_line_bytes, so a peer that
# never sends a newline cannot grow the daemon-socket parser without bound.
# Discharged by `ay` (the Trust SAT/SMT solver) on hand-encoded SMT-LIB2 —
# Trust, NOT kani. Run: `bash verify.sh`. Exits 0 iff every obligation gets its
# expected verdict (or ay is absent, in which case the bundle is SKIPPED).
#
# OBLIGATIONS:
#   oom_buffer_le_max         unsat  guard passed + no wrap => buffer+segment <= max
#   oom_no_wrap               unsat  buffer<=max<=2^63 & segment<=isize::MAX => no wrap
#   oom_nonvacuity_sat        sat    buffer reaches exactly max (bound is tight)
#   oom_catches_unguarded_sat sat    without the guard, buffer > max is reachable
set -u
# ay: $AY, else PATH (the atpkg-managed Trust toolchain). Never a Trust checkout's build/ —
# the compiler repo's exclusive build output, whose ay goes stale. An explicit
# $AY that does not run is an error, never a fall-through to another solver.
if [ -n "${AY:-}" ]; then
  "$AY" --version >/dev/null 2>&1 || { echo "FAIL: \$AY=$AY is not a runnable ay (oom_bound not checked)"; exit 1; }
else
  AY="$(command -v ay 2>/dev/null || true)"
  if [ -n "$AY" ] && ! "$AY" --version >/dev/null 2>&1; then AY=""; fi
fi
[ -n "$AY" ] || { echo "SKIP: no runnable ay found (oom_bound not checked)"; exit 0; }
echo "ay = $AY"
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
expect() { # <file> <sat|unsat>
  local f="$DIR/$1" want="$2" got
  got=$("$AY" solve "$f" -t:120000 2>/dev/null | grep -iE '^(sat|unsat|unknown)$' | head -1 | tr '[:upper:]' '[:lower:]')
  if [ "$got" = "$want" ]; then printf '  PASS  %-30s %s\n' "$1" "$got"; return 0
  else printf '  FAIL  %-30s got=%s want=%s\n' "$1" "${got:-<none>}" "$want"; return 1; fi
}
echo "oom_bound — NDJSON buffer <= max_line_bytes (ay):"
rc=0
expect oom_buffer_le_max.smt2         unsat || rc=1
expect oom_no_wrap.smt2               unsat || rc=1
expect oom_nonvacuity_sat.smt2        sat   || rc=1
expect oom_catches_unguarded_sat.smt2 sat   || rc=1
if [ "$rc" = 0 ]; then echo "oom_bound: ALL OBLIGATIONS DISCHARGED ✓"; else echo "oom_bound: FAILED ✗"; fi
exit "$rc"
