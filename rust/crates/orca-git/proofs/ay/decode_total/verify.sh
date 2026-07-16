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
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Solver ladder + the opt-in AY_SOLVER=z3 portability mode live in ../resolve-solver.sh.
. "$DIR/../resolve-solver.sh"
if [ -z "$SOLVER_BIN" ]; then
  if [ "$SOLVER_KIND" = "z3" ]; then
    echo "FAIL: AY_SOLVER=z3 requested but no runnable z3"
    exit 1
  fi
  echo "SKIP: no runnable ay found (decode_total not checked)"
  exit 0
fi
solver_banner
expect() { # <file> <sat|unsat>
  local f="$DIR/$1" want="$2" got
  got=$(solve_verdict "$f")
  if [ "$got" = "$want" ]; then printf '  PASS  %-42s %s\n' "$1" "$got"; return 0
  else printf '  FAIL  %-42s got=%s want=%s\n' "$1" "${got:-<none>}" "$want"; return 1; fi
}
echo "decode_total — octal escape decode is total ($SOLVER_KIND):"
rc=0
expect decode_octal_total.smt2                    unsat || rc=1
expect decode_octal_mask_matches_uint8.smt2       unsat || rc=1
expect decode_octal_wrap_reachable_sat.smt2       sat   || rc=1
if [ "$rc" = 0 ]; then echo "decode_total: ALL OBLIGATIONS DISCHARGED ✓"; else echo "decode_total: FAILED ✗"; fi
exit "$rc"
