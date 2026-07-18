#!/usr/bin/env bash
# orc-electron fork bootstrap — checkout + build the Electron fork at orc's pinned
# version. DELIBERATE, gauntlet-gated op: ~30-60GB, multi-hour. Run ONLY when a Rung-3/4
# item is scheduled (see README) — the Phase-0 kill-check retired the origin-isolation
# item on Electron 43, so there is currently NO reason to run this. Guarded by an
# explicit --i-mean-it flag so it is never kicked by accident or by an agent in passing.
set -euo pipefail

PIN="v43.1.0"                       # matches package.json electron ^43.1.0 (Chromium 150)
ROOT="${ORC_ELECTRON_ROOT:-$HOME/orc-electron-src}"
DEPOT="${ROOT}/depot_tools"

if [[ "${1:-}" != "--i-mean-it" ]]; then
  cat <<EOF
orc-electron fork bootstrap (pin ${PIN})
This is a multi-hour, ~30-60GB checkout+build. It is NOT needed right now:
the Phase-0 kill-check (run-killcheck.mjs) found STOCK-RUNG-2-SUFFICES on Electron 43,
so the origin-isolation patch is unnecessary. Only run this for a scheduled macOS
low-latency-canvas / component-stripping / V8-snapshot / PGO build.
Re-run with:  bash tools/orc-electron/bootstrap-fork.sh --i-mean-it
EOF
  exit 2
fi

mkdir -p "$ROOT"
# 1. depot_tools
if [[ ! -d "$DEPOT" ]]; then
  git clone https://chromium.googlesource.com/chromium/tools/depot_tools.git "$DEPOT"
fi
export PATH="${DEPOT}:${PATH}"

# 2. sccache for ~3x cached rebuilds across the treadmill (Postman/ungoogled precedent)
if ! command -v sccache >/dev/null 2>&1; then
  echo "WARN: sccache not found — install it (brew install sccache) for cached rebuilds." >&2
fi
export CC_WRAPPER="${CC_WRAPPER:-sccache}"

# 3. electron checkout at the pinned tag (pulls the matching Chromium via gclient)
cd "$ROOT"
if [[ ! -f .gclient ]]; then
  gclient config --name "src/electron" --unmanaged https://github.com/electron/electron.git
fi
gclient sync --with_branch_heads --with_tags --revision "src/electron@${PIN}"

# 4. GN out dir. Patches live in ../patches and are applied on top of src/electron/patches.
cd src/electron
echo "Checkout ready at ${ROOT}. Next: apply tools/orc-electron/patches/*, then:"
echo "  gn gen out/Release --args='import(\"//electron/build/args/release.gn\") cc_wrapper=\"sccache\" is_component_build=false'"
echo "  ninja -C out/Release electron"
echo "Then verify with:  pnpm gauntlet   (the fork must pass the same gate as stock)."
