import { spawn } from 'node:child_process'

// Worker-ON e2e mode: run a curated core subset against the SHIPPED aterm default —
// the shared render worker left ON. ORCA_E2E_ATERM_WORKER=1 tells the orca-app
// fixture NOT to force window.__atermWorkerRender=false, so panes take the same
// worker path production users get (the rest of the suite still forces it off
// because most specs assert via in-process canvas/GPU internals).
//
// CURATED (assert user-visible behavior, valid on both render paths):
//   - aterm-clipboard      — bracketed-paste bytes + OSC-52 via main-process spies
//   - aterm-query-replies  — CPR/DA1/OSC-11/14t/16t round-trip to the PTY
//   - aterm-a11y           — ARIA live region mirrors rendered output
//   - aterm-selection      — double/triple-click selection text (polled reads)
// EXCLUDED (in-process-only assertion surfaces — the worker owns the transferred
// canvas, so main-thread getContext/toDataURL throw; do not add back without a
// worker-compatible assertion path):
//   - aterm-default-on-shim — paint proofs are main-thread canvas pixel diffs + toDataURL
//   - aterm-retheme         — reads the repaint from the grid canvas's own 2d pixels
//   - aterm-search          — asserts findMatches' synchronous count + overlay pixel
//                             diffs; worker-path search is covered by aterm-worker-search
// The 3 dedicated worker specs run too, so this mode can't go green while they regress.

const CURATED_SPECS = [
  'tests/e2e/aterm-clipboard.spec.ts',
  'tests/e2e/aterm-query-replies.spec.ts',
  'tests/e2e/aterm-a11y.spec.ts',
  'tests/e2e/aterm-selection.spec.ts',
  'tests/e2e/aterm-worker-render.spec.ts',
  'tests/e2e/aterm-worker-gpu-render.spec.ts',
  'tests/e2e/aterm-worker-search.spec.ts'
]

const npxCommand = process.platform === 'win32' ? 'npx.cmd' : 'npx'

const extraArgs = process.argv.slice(2)
if (extraArgs[0] === '--') {
  extraArgs.shift()
}

const child = spawn(
  npxCommand,
  [
    'playwright',
    'test',
    ...CURATED_SPECS,
    '--config',
    'tests/playwright.config.ts',
    '--project',
    'electron-headless',
    '--workers=2',
    ...extraArgs
  ],
  {
    stdio: 'inherit',
    env: { ...process.env, ORCA_E2E_ATERM_WORKER: '1' }
  }
)

child.on('exit', (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal)
    return
  }
  process.exit(code ?? 1)
})
