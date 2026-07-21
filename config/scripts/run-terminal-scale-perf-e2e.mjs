import { spawn } from 'node:child_process'
import { createRequire } from 'node:module'
import { dirname, resolve } from 'node:path'
import { normalizeChildColorEnv } from './child-process-color-env.mjs'

const require = createRequire(import.meta.url)
const playwrightCli = resolve(dirname(require.resolve('@playwright/test/package.json')), 'cli.js')

const env = {
  ...normalizeChildColorEnv(),
  ORCA_E2E_OPENCODE_SCALE_PANES: process.env.ORCA_E2E_OPENCODE_SCALE_PANES ?? '10,25,50,100',
  ORCA_E2E_OPENCODE_SCALE_CROSS_WORKSPACE_PANES:
    process.env.ORCA_E2E_OPENCODE_SCALE_CROSS_WORKSPACE_PANES ?? '10,25,50,100',
  ORCA_E2E_OPENCODE_SCALE_PRESSURE_PANES:
    process.env.ORCA_E2E_OPENCODE_SCALE_PRESSURE_PANES ?? '25,50',
  ORCA_E2E_OPENCODE_SCALE_HIDDEN_PRESSURE_PANES:
    process.env.ORCA_E2E_OPENCODE_SCALE_HIDDEN_PRESSURE_PANES ?? '25',
  ORCA_E2E_OPENCODE_FRAME_COUNT: process.env.ORCA_E2E_OPENCODE_FRAME_COUNT ?? '60'
}
const extraArgs = process.argv.slice(2)
if (extraArgs[0] === '--') {
  extraArgs.shift()
}

const child = spawn(
  process.execPath,
  [
    playwrightCli,
    'test',
    'tests/e2e/artificial-opencode-terminal-load.spec.ts',
    '--config',
    'tests/playwright.config.ts',
    '--project',
    'electron-headless',
    '--workers=1',
    ...extraArgs
  ],
  {
    stdio: 'inherit',
    env
  }
)

child.on('exit', (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal)
    return
  }
  process.exit(code ?? 1)
})
