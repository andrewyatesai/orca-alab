#!/usr/bin/env bash
# Copyright 2026 The aterm Authors
# SPDX-License-Identifier: Apache-2.0
#
# Re-checkable certificate for initiative A9: the appearance-aware tab strip
# (crates/aterm-gui/src/tab_bar.rs strip_colors / bg_is_light) plus the bundled
# light colour schemes (crates/aterm-types/src/scheme.rs). Discharged by `ay`
# (the Trust SAT/SMT solver) on hand-encoded SMT-LIB2 — Trust, NOT kani. Run:
# `bash verify.sh`. Exits 0 iff every obligation gets its expected verdict.
#
# THEOREMS (negation asserted; UNSAT == proved for all inputs in scope):
#   partition_dark            unsat  every Appearance::Dark builtin bg classifies DARK
#   partition_light           unsat  every Appearance::Light builtin bg classifies LIGHT
#   dark_factors_unchanged    unsat  whole dark region resolves the legacy (16,40)
#                                    factors => dark output is BYTE-IDENTICAL (no regress)
#   active_distinct           unsat  the active card != the body for every builtin
#   raise_direction           unsat  active card raises per appearance (dark up / light down)
#   selection_legible         unsat  WCAG contrast(fg,selection) >= 3.0 for every builtin
#                                    (sound rational luminance bounds; division-free)
# CONTROLS (the prove-AND-catch discipline; SAT == non-vacuous):
#   partition_nonvacuity_sat            a real light builtin exists (set encoding non-empty)
#   catches_threshold_regression_sat    a lowered threshold misclassifies Nord (margin real)
#   legible_catches_false_floor_sat     a too-strong floor (>=4.0) fails the tight builtin
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
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
expect() { # <file> <sat|unsat>
  local f="$DIR/$1" want="$2" got
  got=$("$AY" solve "$f" -t:120000 2>/dev/null | grep -viE '^c ay\.|^\(' | tr -d '[:space:]' | tr '[:upper:]' '[:lower:]')
  if [ "$got" = "$want" ]; then printf '  PASS  %-36s %s\n' "$1" "$got"; return 0
  else printf '  FAIL  %-36s got=%s want=%s\n' "$1" "${got:-<none>}" "$want"; return 1; fi
}
echo "A9 — appearance-aware tab strip + light schemes (ay):"
rc=0
expect partition_dark.smt2                   unsat || rc=1
expect partition_light.smt2                  unsat || rc=1
expect dark_factors_unchanged.smt2           unsat || rc=1
expect active_distinct.smt2                  unsat || rc=1
expect raise_direction.smt2                  unsat || rc=1
expect selection_legible.smt2                unsat || rc=1
expect partition_nonvacuity_sat.smt2         sat   || rc=1
expect catches_threshold_regression_sat.smt2 sat   || rc=1
expect legible_catches_false_floor_sat.smt2  sat   || rc=1
if [ "$rc" = 0 ]; then echo "A9: ALL OBLIGATIONS DISCHARGED ✓"; else echo "A9: FAILED ✗"; fi
exit "$rc"
