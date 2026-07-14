#!/usr/bin/env bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
#
# Re-checkable certificate for the C-quote octal-escape decode TOTALITY (see
# rust/PROOF_CARRYING_PERFORMANCE.md) under the byte-accumulation arm: every 1-3
# digit octal escape yields exactly one u8 (parse can't overflow, `(v & 0xFF) as
# u8` fits), so the decoder never panics and never drops an escape; and the mask
# reproduces the TS Uint8Array wrap byte-for-byte. Discharged by `ay` (the Trust
# SAT/SMT solver) — Trust, NOT kani. Run: `bash verify.sh`. Exits 0 iff every
# obligation gets its verdict (or ay is absent, in which case SKIPPED, not failed).
#
# OBLIGATIONS:
#   decode_octal_total              unsat  di<=7 => v<=511 AND (v&0xFF)<=255 (total, no drop)
#   decode_octal_mask_matches_uint8 unsat  (v&0xFF) == (v mod 256) (byte-identical to the TS)
#   decode_octal_wrap_reachable_sat sat    v>255 reachable & the mask wraps (non-vacuity)
set -u
AY=""
for c in \
  "$HOME/.cargo/bin/ay" \
  "$HOME/trust/build/host/stage2/bin/ay" \
  "$HOME/trust/build/aarch64-apple-darwin/stage3-tools-bin/aarch64-apple-darwin/ay" \
  "$HOME/trust/build/aarch64-apple-darwin/stage2-tools-bin/aarch64-apple-darwin/ay" ; do
  if "$c" --version >/dev/null 2>&1; then AY="$c"; break; fi
done
[ -n "$AY" ] || { echo "SKIP: no runnable ay found (decode_total not checked)"; exit 0; }
echo "ay = $AY"
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
expect() { # <file> <sat|unsat>
  local f="$DIR/$1" want="$2" got
  got=$("$AY" solve "$f" -t:120000 2>/dev/null | grep -iE '^(sat|unsat|unknown)$' | head -1 | tr '[:upper:]' '[:lower:]')
  if [ "$got" = "$want" ]; then printf '  PASS  %-42s %s\n' "$1" "$got"; return 0
  else printf '  FAIL  %-42s got=%s want=%s\n' "$1" "${got:-<none>}" "$want"; return 1; fi
}
echo "decode_total — octal escape decode is total (ay):"
rc=0
expect decode_octal_total.smt2                    unsat || rc=1
expect decode_octal_mask_matches_uint8.smt2       unsat || rc=1
expect decode_octal_wrap_reachable_sat.smt2       sat   || rc=1
if [ "$rc" = 0 ]; then echo "decode_total: ALL OBLIGATIONS DISCHARGED ✓"; else echo "decode_total: FAILED ✗"; fi
exit "$rc"
