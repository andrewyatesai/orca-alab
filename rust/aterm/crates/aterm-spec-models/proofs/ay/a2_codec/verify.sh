#!/usr/bin/env bash
# Copyright 2026 The aterm Authors
# SPDX-License-Identifier: Apache-2.0
#
# Re-checkable certificate for initiative A2: the base64/hex codec decode path is
# TOTAL (never panics) and the base64 ENCODER's output is provably ASCII (which is
# what licenses its unsafe String::from_utf8_unchecked). Discharged by `ay` (the
# Trust SAT/SMT solver) on hand-encoded SMT-LIB2 — Trust, NOT kani.
# Run: `bash verify.sh`. Exits 0 iff every obligation gets its expected verdict.
#
# OBLIGATIONS:
#   decode_table_index_inbounds        unsat  byte as usize < 256 (table lookup no panic)
#   encoder_alphabet_index_inbounds    unsat  (n>>k)&0x3F < 64    (alphabet lookup no panic)
#   encoder_accumulator_no_overflow    unsat  (c0<<16)|(c1<<8)|c2 <= 0xFFFFFF (u32 no overflow)
#   encoder_output_ascii               unsat  every emitted byte < 128 (from_utf8_unchecked sound)
#   hex_nibble_no_underflow            unsat  each match-arm subtraction in-range, nibble < 16
#   decode_total_witness_sat           sat    decode_byte Err + Ok branches both reachable (total)
#   catches_false_alphabet_bound_sat   sat    masked index 63 reachable => 64 is the LEAST bound
#   hex_total_witness_sat              sat    non-hex byte hits `_` arm => Err (total, not panic)
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
  if [ "$got" = "$want" ]; then printf '  PASS  %-38s %s\n' "$1" "$got"; return 0
  else printf '  FAIL  %-38s got=%s want=%s\n' "$1" "${got:-<none>}" "$want"; return 1; fi
}
echo "A2 — base64/hex codec: decode-never-panics + encoder-output-is-ASCII (ay):"
rc=0
expect decode_table_index_inbounds.smt2        unsat || rc=1
expect encoder_alphabet_index_inbounds.smt2    unsat || rc=1
expect encoder_accumulator_no_overflow.smt2    unsat || rc=1
expect encoder_output_ascii.smt2               unsat || rc=1
expect hex_nibble_no_underflow.smt2            unsat || rc=1
expect decode_total_witness_sat.smt2           sat   || rc=1
expect catches_false_alphabet_bound_sat.smt2   sat   || rc=1
expect hex_total_witness_sat.smt2              sat   || rc=1
if [ "$rc" = 0 ]; then echo "A2: ALL OBLIGATIONS DISCHARGED ✓"; else echo "A2: FAILED ✗"; fi
exit "$rc"
