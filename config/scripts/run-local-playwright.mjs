#!/usr/bin/env node

import { spawnSync } from 'node:child_process'
import { createRequire } from 'node:module'
import { dirname, resolve } from 'node:path'
import { normalizeChildColorEnv } from './child-process-color-env.mjs'

const require = createRequire(import.meta.url)
const playwrightCli = resolve(dirname(require.resolve('@playwright/test/package.json')), 'cli.js')
const result = spawnSync(process.execPath, [playwrightCli, ...process.argv.slice(2)], {
  stdio: 'inherit',
  env: normalizeChildColorEnv()
})

if (result.signal) {
  process.kill(process.pid, result.signal)
}
process.exit(result.status ?? (result.error ? 1 : 0))
