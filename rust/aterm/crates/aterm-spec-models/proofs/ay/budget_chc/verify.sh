#!/usr/bin/env bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
#
# Re-checkable certificate for initiative A8: the aterm scrollback evicting-push
# BYTE-BUDGET inductive invariant (OOM-impossibility theorem).
# Discharged by `ay` (SAT/SMT/CHC solver) on hand-encoded SMT-LIB2 CHC (HORN) —
# no trust-mc needed. Run: `bash verify.sh`. Exits 0 iff every obligation gets
# its expected verdict.
#
# CHC POLARITY (verified on this box):
#   sat   = an inductive invariant EXISTS = SAFE (ay prints the synthesized invariant)
#   unsat = the error state is REACHABLE = UNSAFE (ay prints a counterexample trace)
#
# OBLIGATIONS:
#   budget_safe                       sat    faithful evicting-push: b <= n+k holds
#                                            (ay synthesizes b>=0 /\ n>=1 /\ k>=1 /\ b<=n+k)
#   budget_buggy                      unsat  eviction+guard removed => OOM b>n+k reachable
#                                            (prove-and-catch: the theorem is non-vacuous)
#   budget_catches_false_unconditional unsat catches the FALSE unconditional bound b<=n
#                                            (a single push transiently overshoots)
#   budget_bound_is_tight             unsat  n+k is the LEAST upper bound (b<=n+k-1 fails)
set -u

# Locate a runnable ay (the ~/.cargo/bin/ay symlink can dangle while the trust
# sysroot rebuilds; fall back to in-tree bootstrap outputs — mirrors A5).
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
  # Verdict extractor from SOURCE_FACTS: first bare sat/unsat/unknown line.
  got=$("$AY" solve "$f" 2>/dev/null | grep -iE '^(sat|unsat|unknown)$' | head -1 | tr '[:upper:]' '[:lower:]')
  if [ "$got" = "$want" ]; then
    printf '  PASS  %-38s %s\n' "$1" "$got"; return 0
  else
    printf '  FAIL  %-38s got=%s want=%s\n' "$1" "${got:-<none>}" "$want"; return 1
  fi
}

echo "A8 — scrollback evicting-push byte-budget inductive invariant (ay-CHC):"
rc=0
expect budget_safe.smt2                        sat   || rc=1
expect budget_buggy.smt2                       unsat || rc=1
expect budget_catches_false_unconditional.smt2 unsat || rc=1
expect budget_bound_is_tight.smt2              unsat || rc=1
if [ "$rc" = 0 ]; then echo "A8: ALL OBLIGATIONS DISCHARGED ✓"; else echo "A8: FAILED ✗"; fi
exit "$rc"
