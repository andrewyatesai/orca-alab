# orc-electron patch series

Patches carried on top of Electron's own `patches/` (over Chromium), applied by
`bootstrap-fork.sh` before the GN build. Value-ordered; each entered on evidence.

| # | Patch | Status | Gate |
|---|---|---|---|
| — | **origin-isolation** (grant `crossOriginIsolated` to the app scheme, PR #50789 seam) | **RETIRED on E43** | Phase-0 kill-check → `STOCK-RUNG-2-SUFFICES` (see ../README.md). Re-enters only if a future major flips the verdict to `FORK-PATCH-JUSTIFIED`. |
| 1 | **macOS low-latency canvas** — carry the present-path patch Chromium never finished (1–2 compositor frames off keystroke present) | planned | a `perf_harness`/typometer win on macOS present latency vs stock, byte-identical render |
| 2 | **component stripping** (spellcheck/PDF/printing/translate out) + PartitionAlloc tuning | planned | bundle-size + RSS deltas; full app parity via `pnpm gauntlet` |
| 3 | **pointer-compression-off whale variant** (8–16GB renderer heap) | planned | retires the heavy-session crash class; variant build, not default |
| 4 | **custom V8 startup snapshot** with the app graph baked in | planned | cold-start delta; snapshot determinism |

Rules: every patch is minimal, rationale-commented, and rebases cleanly each 8-week
major. A patch that fails `pnpm gauntlet` on any OS×arch does not ship. Prefer landing
the change upstream in Electron/Chromium when it is general — a carried patch is a cost.
