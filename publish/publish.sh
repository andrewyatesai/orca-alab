#!/usr/bin/env bash
# Shim -> the `pub` CLI in the sibling `publication` repo. Policy lives HERE
# (manifest.txt, config.sh, transforms.sh, ...); mechanism lives there.
set -euo pipefail
HERE="$(cd "$(dirname "$0")/.." && pwd)"
ENGINE="${PUBLICATION_DIR:-$HERE/../publication}"
[ -x "$ENGINE/bin/pub" ] || { echo "FAIL: publication repo not found at $ENGINE — clone it as a sibling, or set PUBLICATION_DIR" >&2; exit 1; }
cd "$HERE" && exec "$ENGINE/bin/pub" "$@"
