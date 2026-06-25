#!/usr/bin/env bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
#
# Re-checkable certificate for initiative A5 (PROOF_CARRYING_PERFORMANCE.md):
# the CPU coverage-blend `blend()` (crates/aterm-render/src/lib.rs:2257-2266).
# Discharged by `ay` (SAT/SMT/CHC solver) on hand-encoded SMT-LIB2 — no trust-mc
# needed. Run: `bash verify.sh`. Exits 0 iff every obligation gets its expected verdict.
#
# THEOREMS (negation asserted; UNSAT == proved for all bg,fg,t in 0..=255):
#   blend_endpoint_exact   t in {0,255} => mix is bit-exact (==bg / ==fg)        [A5 committed scope]
#   blend_in_gamut_caseA   bg<=fg  => min(bg,fg) <= mix <= max(bg,fg)            [A5+ no-overshoot]
#   blend_in_gamut_caseB   fg<=bg  => min(bg,fg) <= mix <= max(bg,fg)            [A5+ no-overshoot]
#   blend_numerator_nowrap numerator <= 130050 < 2^18  (justifies the 18-bit model fidelity)
# CONTROLS (the prove-AND-catch discipline; SAT == non-vacuous):
#   blend_nonvacuity_sat       a real interior value exists (mix=128)
#   blend_catches_false_bound  the checker catches a deliberately false bound (mix<=200)
#
# A5+ no-overshoot (cases A+B) => 0 <= mix <= 255 => the UNMASKED packing
# `(mix_r<<16)|(mix_g<<8)|mix_b` at lib.rs:2266 cannot bleed across channels:
# the absence of a `& 0xff` mask is a discharged theorem, not a comment.
set -u
# Locate a runnable ay. The canonical ~/.cargo/bin/ay symlink can dangle while the
# trust sysroot rebuilds, so fall back to the in-tree bootstrap outputs.
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

# Resolve the bundle dir so the gate works from ANY cwd (not just from here).
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

expect() { # <file> <SAT|UNSAT>
  local f="$1" want="$2"
  local got
  got=$("$AY" solve "$DIR/$f" -t:120000 2>/dev/null | grep -viE '^c ay\.|^\(' | tr -d '[:space:]')
  if [ "$got" = "$(echo "$want" | tr '[:upper:]' '[:lower:]')" ]; then
    printf '  PASS  %-30s %s\n' "$f" "$got"; return 0
  else
    printf '  FAIL  %-30s got=%s want=%s\n' "$f" "${got:-<none>}" "$want"; return 1
  fi
}

echo "A5 — coverage-blend proof bundle (ay):"
rc=0
expect blend_endpoint_exact.smt2     UNSAT || rc=1
expect blend_in_gamut_caseA.smt2     UNSAT || rc=1
expect blend_in_gamut_caseB.smt2     UNSAT || rc=1
expect blend_numerator_nowrap.smt2   UNSAT || rc=1
expect blend_nonvacuity_sat.smt2     SAT   || rc=1
expect blend_catches_false_bound.smt2 SAT  || rc=1
if [ "$rc" = 0 ]; then echo "A5: ALL OBLIGATIONS DISCHARGED ✓"; else echo "A5: FAILED ✗"; fi
exit "$rc"
