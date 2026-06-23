#!/usr/bin/env bash
# Copyright 2026 The aterm Authors
# SPDX-License-Identifier: Apache-2.0
#
# Re-checkable certificate for initiative A7: the legacy keyboard SHIFT map is
# EFFECTIVE (Shift changes every shiftable key) and TOTAL into printable ASCII —
# the property the "Shift doesn't work" regression (aterm a2742d7) violated, where
# `to_ascii_uppercase` no-op'd on every digit/symbol so Shift+2 emitted '2' not
# '@'. Discharged by `ay` (the Trust SAT/SMT solver) on hand-encoded SMT-LIB2 —
# Trust, NOT kani. Run: `bash verify.sh`. Exits 0 iff every obligation gets its
# expected verdict.
#
# OBLIGATIONS:
#   shift_is_effective                 unsat  for all shiftable c, ShiftSpec(c) != c
#   shift_glyph_is_printable           unsat  for all shiftable c, 0x20 <= ShiftSpec(c) <= 0x7e
#   catches_uppercase_bug_sat          sat    the buggy to_ascii_uppercase map != spec (bug caught)
#   shift_effective_nonvacuity_sat     sat    a shiftable key is genuinely moved (non-vacuous)
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
echo "A7 — legacy keyboard shift: effective + total-into-printable (ay):"
rc=0
expect shift_is_effective.smt2              unsat || rc=1
expect shift_glyph_is_printable.smt2        unsat || rc=1
expect catches_uppercase_bug_sat.smt2       sat   || rc=1
expect shift_effective_nonvacuity_sat.smt2  sat   || rc=1
if [ "$rc" = 0 ]; then echo "A7: ALL OBLIGATIONS DISCHARGED ✓"; else echo "A7: FAILED ✗"; fi
exit "$rc"
