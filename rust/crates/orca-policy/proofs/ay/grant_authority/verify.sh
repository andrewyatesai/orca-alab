#!/usr/bin/env bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
#
# Re-checkable certificate for orca-policy's fleet-grant authorization decision
# (see rust/PROOF_CARRYING_PERFORMANCE.md for the proof-boundary contract).
# decide_fleet_grant is what stands between a worker with shell access and every
# other pane on the machine, so its refusals are proved rather than tested.
#
# Discharged by `ay` (the Trust SAT/SMT solver) on hand-encoded SMT-LIB2 — Trust,
# NOT kani. Run: `bash verify.sh`. Exits 0 iff every obligation gets its expected
# verdict (or ay is absent, in which case the bundle is SKIPPED, not passed).
#
# OBLIGATIONS:
#   grant_revoked_never_allowed            unsat  revocation cannot be out-voted
#   grant_expired_never_allowed            unsat  expiry is inclusive and final
#   grant_unknown_incarnation_fails_closed unsat  pinned + unknown => deny
#   grant_nonvacuity_sat                   sat    `allowed` is reachable at all
#   grant_catches_dropped_revoke_check_sat sat    the revoked guard is load-bearing
#
# The last two are why the first three mean something: without non-vacuity the
# "never allowed" claims hold trivially against a function that allows nothing,
# and without the catch-control they might be credited to a guard that is not
# doing the work.
set -u
# ay: $AY, else PATH (the atpkg-managed Trust toolchain). Never ~/trust/build —
# the compiler repo's exclusive build output, whose ay goes stale.
AY_REQUESTED="${AY:-}"
AY=""
for c in "$AY_REQUESTED" "$(command -v ay 2>/dev/null || true)"; do
  if [ -n "$c" ] && "$c" --version >/dev/null 2>&1; then AY="$c"; break; fi
done
[ -n "$AY" ] || { echo "SKIP: no runnable ay found (grant_authority not checked)"; exit 0; }
echo "ay = $AY"
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
expect() { # <file> <sat|unsat>
  local f="$DIR/$1" want="$2" got
  got=$("$AY" solve "$f" -t:120000 2>/dev/null | grep -iE '^(sat|unsat|unknown)$' | head -1 | tr '[:upper:]' '[:lower:]')
  if [ "$got" = "$want" ]; then printf '  PASS  %-42s %s\n' "$1" "$got"; return 0
  else printf '  FAIL  %-42s got=%s want=%s\n' "$1" "${got:-<none>}" "$want"; return 1; fi
}
echo "grant_authority — fleet-grant refusals are unconditional (ay):"
rc=0
expect grant_revoked_never_allowed.smt2            unsat || rc=1
expect grant_expired_never_allowed.smt2            unsat || rc=1
expect grant_unknown_incarnation_fails_closed.smt2 unsat || rc=1
expect grant_nonvacuity_sat.smt2                   sat   || rc=1
expect grant_catches_dropped_revoke_check_sat.smt2 sat   || rc=1
if [ "$rc" = 0 ]; then echo "grant_authority: ALL OBLIGATIONS DISCHARGED ✓"; else echo "grant_authority: FAILED ✗"; fi
exit "$rc"
