#!/usr/bin/env bash
# Copyright 2026 Andrew Yates
# SPDX-License-Identifier: Apache-2.0
#
# Aggregator for the orca-git unified status/diff parser proof bundles. orca-git
# is OUTSIDE aterm's verification gate, so it carries its OWN proofs/ay tree (see
# rust/PROOF_CARRYING_PERFORMANCE.md for the proof-boundary contract). Runs each
# bundle's verify.sh and reports an aggregate verdict. Each bundle SKIPS (exit 0)
# rather than fails when `ay` is absent, so this gate is green on a box without ay.
set -u
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
rc=0
for bundle in cap_invariant decode_total line_scan_bounds; do
  echo "=== $bundle ==="
  bash "$DIR/$bundle/verify.sh" || rc=1
  echo
done
if [ "$rc" = 0 ]; then echo "orca-git proofs/ay: ALL BUNDLES DISCHARGED (or skipped) ✓"; else echo "orca-git proofs/ay: FAILED ✗"; fi
exit "$rc"
