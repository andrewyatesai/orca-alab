#!/usr/bin/env bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
#
# Re-checkable certificate for initiative A6: the GPU atlas texture HEIGHT is clamped
# to the device's max 2D texture dimension, turning the oversized-texture device-abort
# (lost-device DoS) into an IMPOSSIBILITY for the proven (height/headroom) scope.
# Discharged by `ay` (the Trust SAT/SMT solver) on hand-encoded SMT-LIB2 — Trust,
# NOT kani. Run: `bash verify.sh`. Exits 0 iff every obligation gets its verdict.
#
# POLARITY (plain QF_BV SMT, NOT CHC): each .smt2 asserts the NEGATION of its theorem.
#   unsat = theorem holds for ALL inputs in the modeled domain.
#   sat   = a witness / counterexample exists.
#
# OBLIGATIONS:
#   height_clamp_le_limit     unsat  (h+HEADROOM).min(max) <= max  for all h, max
#                                    (the .min guarantees it; device-abort impossible)
#   height_no_overflow        unsat  under packer bound (h<=max) AND device margin
#                                    (max<=u32::MAX-HEADROOM), h+HEADROOM does not wrap
#                                    => the clamp sees the TRUE (non-wrapped) value
#   width_within_limit        unsat  under stated assume(max>=2048, wgpu downlevel min),
#                                    ATLAS_WIDTH (1024) <= max  (documented precondition)
#   clamp_is_load_bearing     sat    UNCLAMPED h+HEADROOM CAN exceed max (device-abort
#                                    reachable) => the .min is load-bearing, not dead
#   nonvacuity_interior_sat   sat    tex_h takes a real interior value (clamp a no-op,
#                                    headroom genuinely allocated) => model not degenerate
set -u

# Locate a runnable ay (the ~/.cargo/bin/ay symlink can dangle while the trust
# sysroot rebuilds; fall back to in-tree bootstrap outputs — mirrors A1/A8).
AY=""
for c in \
  "$HOME/.cargo/bin/ay" \
  "$HOME/trust/build/host/stage2/bin/ay" \
  "$HOME/trust/build/aarch64-apple-darwin/stage3-tools-bin/aarch64-apple-darwin/ay" \
  "$HOME/trust/build/aarch64-apple-darwin/stage2-tools-bin/aarch64-apple-darwin/ay" ; do
  if "$c" --version >/dev/null 2>&1; then AY="$c"; break; fi
done
[ -n "$AY" ] || { echo "FATAL: no runnable ay found (cargo bin / trust stage2|stage3 tools-bin)"; exit 2; }
echo "ay = $AY"

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

expect() { # <file> <sat|unsat>
  local f="$DIR/$1" want="$2" got
  got=$("$AY" solve "$f" -t:60000 2>/dev/null | grep -iE '^(sat|unsat|unknown)$' | head -1 | tr '[:upper:]' '[:lower:]')
  if [ "$got" = "$want" ]; then
    printf '  PASS  %-30s %s\n' "$1" "$got"; return 0
  else
    printf '  FAIL  %-30s got=%s want=%s\n' "$1" "${got:-<none>}" "$want"; return 1
  fi
}

echo "A6 — GPU atlas texture-height device-limit clamp (ay):"
rc=0
expect height_clamp_le_limit.smt2   unsat || rc=1
expect height_no_overflow.smt2      unsat || rc=1
expect width_within_limit.smt2      unsat || rc=1
expect clamp_is_load_bearing.smt2   sat   || rc=1
expect nonvacuity_interior_sat.smt2 sat   || rc=1
if [ "$rc" = 0 ]; then echo "A6: ALL OBLIGATIONS DISCHARGED ✓"; else echo "A6: FAILED ✗"; fi
exit "$rc"
