#!/usr/bin/env bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
#
# Re-checkable certificate for the orca-git unified status parser's CAP invariant
# (see rust/PROOF_CARRYING_PERFORMANCE.md for the proof-boundary contract): the
# parser EMITS <= limit entries and BUFFERS <= limit+2. Discharged by `ay` (the
# Trust SAT/SMT solver) on hand-encoded SMT-LIB2 — Trust, NOT kani.
# Run: `bash verify.sh`. Exits 0 iff every obligation gets its verdict (or ay is
# absent, in which case the bundle is SKIPPED, not failed).
#
# OBLIGATIONS:
#   cap_emit_le_limit              unsat  min(count,limit) <= limit (unconditional)
#   cap_buffer_le_limit_plus_2     unsat  under P1 (count<=limit pre-line) & <=2/line: c+k <= limit+2
#   cap_nonvacuity_sat             sat    c=limit,k=2 reaches limit+2 (bound is tight)
#   cap_catches_false_tight_sat    sat    buffer > limit+1 reachable (false limit+1 bound caught)
set -u
AY=""
for c in \
  "$HOME/.cargo/bin/ay" \
  "$HOME/trust/build/host/stage2/bin/ay" \
  "$HOME/trust/build/aarch64-apple-darwin/stage3-tools-bin/aarch64-apple-darwin/ay" \
  "$HOME/trust/build/aarch64-apple-darwin/stage2-tools-bin/aarch64-apple-darwin/ay" ; do
  if "$c" --version >/dev/null 2>&1; then AY="$c"; break; fi
done
[ -n "$AY" ] || { echo "SKIP: no runnable ay found (cap_invariant not checked)"; exit 0; }
echo "ay = $AY"
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
expect() { # <file> <sat|unsat>
  local f="$DIR/$1" want="$2" got
  got=$("$AY" solve "$f" -t:120000 2>/dev/null | grep -iE '^(sat|unsat|unknown)$' | head -1 | tr '[:upper:]' '[:lower:]')
  if [ "$got" = "$want" ]; then printf '  PASS  %-34s %s\n' "$1" "$got"; return 0
  else printf '  FAIL  %-34s got=%s want=%s\n' "$1" "${got:-<none>}" "$want"; return 1; fi
}
echo "cap_invariant — parser emits <= limit, buffers <= limit+2 (ay):"
rc=0
expect cap_emit_le_limit.smt2           unsat || rc=1
expect cap_buffer_le_limit_plus_2.smt2  unsat || rc=1
expect cap_nonvacuity_sat.smt2          sat   || rc=1
expect cap_catches_false_tight_sat.smt2 sat   || rc=1
if [ "$rc" = 0 ]; then echo "cap_invariant: ALL OBLIGATIONS DISCHARGED ✓"; else echo "cap_invariant: FAILED ✗"; fi
exit "$rc"
