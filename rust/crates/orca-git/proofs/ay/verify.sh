#!/usr/bin/env bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
#
# Aggregator for the orca-git unified status/diff parser proof bundles. orca-git
# is OUTSIDE aterm's verification gate, so it carries its OWN proofs/ay tree (see
# rust/PROOF_CARRYING_PERFORMANCE.md for the proof-boundary contract). Runs each
# bundle's verify.sh and reports an aggregate verdict. Each bundle SKIPS (exit 0)
# rather than fails when `ay` is absent, so this gate is green on a box without ay.
#
# Solver: resolve-solver.sh ladder ($AY → PATH → ~/.cargo/bin → trust build dirs).
# `--solver z3` (or AY_SOLVER=z3) re-checks the same bundles with stock z3 as an
# independent portability check; ay remains the toolchain of record.
set -u
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
while [ $# -gt 0 ]; do
  case "$1" in
    --solver)
      AY_SOLVER="${2:?--solver needs a value (ay|z3)}"
      shift 2
      ;;
    --solver=*)
      AY_SOLVER="${1#--solver=}"
      shift
      ;;
    *)
      echo "usage: verify.sh [--solver ay|z3]"
      exit 1
      ;;
  esac
done
export AY_SOLVER="${AY_SOLVER:-ay}"
. "$DIR/resolve-solver.sh"
if [ -z "$SOLVER_BIN" ] && [ "$SOLVER_KIND" = "z3" ]; then
  # z3 mode is an explicit request — a silent green here would be misleading.
  echo "FATAL: --solver z3 requested but no runnable z3 (\$Z3 or PATH)"
  exit 1
fi
if [ -n "$SOLVER_BIN" ]; then
  solver_banner
  # Pin every bundle to the exact binary this aggregator resolved.
  if [ "$SOLVER_KIND" = "ay" ]; then export AY="$SOLVER_BIN"; else export Z3="$SOLVER_BIN"; fi
  echo
fi
rc=0
for bundle in cap_invariant decode_total line_scan_bounds; do
  echo "=== $bundle ==="
  bash "$DIR/$bundle/verify.sh" || rc=1
  echo
done
label=""
if [ "$SOLVER_KIND" = "z3" ]; then label=" [independent z3 portability check — ay is the toolchain of record]"; fi
if [ "$rc" = 0 ]; then echo "orca-git proofs/ay: ALL BUNDLES DISCHARGED (or skipped) ✓$label"; else echo "orca-git proofs/ay: FAILED ✗$label"; fi
exit "$rc"
