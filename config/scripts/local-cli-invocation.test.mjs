import { existsSync, readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, join, resolve } from 'node:path'
import { spawnSync } from 'node:child_process'
import { describe, expect, it } from 'vitest'

const projectDir = resolve(import.meta.dirname, '../..')
const require = createRequire(import.meta.url)

const localCliRunners = [
  'config/scripts/run-aterm-worker-on-e2e.mjs',
  'config/scripts/run-idle-cpu-benchmark.mjs',
  'config/scripts/run-local-playwright.mjs',
  'config/scripts/run-multi-workspace-typing-bench.mjs',
  'config/scripts/run-terminal-scale-perf-e2e.mjs',
  'config/scripts/verify-linux-wayland-gpu-sandbox.mjs',
  'tests/e2e/global-setup.ts',
  'tools/benchmarks/perf-proof-run.mjs',
  'tools/repro-watcher-crash-7547/fixed-child.cjs'
]

describe('local CLI invocation', () => {
  it('does not route installed build and test CLIs through npx', () => {
    for (const relativePath of localCliRunners) {
      const source = readFileSync(join(projectDir, relativePath), 'utf8')
      expect(source, relativePath).not.toMatch(/\bnpx(?:\.cmd)?\b/)
    }
  })

  it('resolves the installed CLI entrypoints used by runners', () => {
    const playwrightPackage = require.resolve('@playwright/test/package.json')
    const electronVitePackage = require.resolve('electron-vite/package.json')

    expect(existsSync(resolve(dirname(playwrightPackage), 'cli.js'))).toBe(true)
    expect(existsSync(resolve(dirname(electronVitePackage), 'bin/electron-vite.js'))).toBe(true)
    const esbuildCli = require.resolve('esbuild/bin/esbuild')
    expect(existsSync(esbuildCli)).toBe(true)

    const esbuildVersion = spawnSync(esbuildCli, ['--version'], { encoding: 'utf8' })
    expect(esbuildVersion.status).toBe(0)
    expect(esbuildVersion.stdout.trim()).toMatch(/^\d+\.\d+\.\d+$/)
  })

  it('uses the pnpm-script PATH for mobile TypeScript utilities', () => {
    const mobilePackage = JSON.parse(readFileSync(join(projectDir, 'mobile/package.json'), 'utf8'))

    expect(mobilePackage.scripts['mock-server']).toBe('tsx scripts/mock-server.ts')
    expect(mobilePackage.scripts['repro:workspace-picker-lag']).toBe(
      'tsx scripts/repro-workspace-picker-lag.ts'
    )
  })

  it('routes package Playwright scripts through the warning-safe local wrapper', () => {
    const rootPackage = JSON.parse(readFileSync(join(projectDir, 'package.json'), 'utf8'))
    const playwrightScripts = Object.entries(rootPackage.scripts).filter(([, command]) =>
      command.includes('playwright')
    )

    expect(playwrightScripts.length).toBeGreaterThan(0)
    for (const [name, command] of playwrightScripts) {
      expect(command, name).not.toMatch(/\bnpx(?:\.cmd)?\s+playwright\b/)
      expect(command, name).not.toMatch(/(?:^|&&\s*)playwright\s+test\b/)
      expect(command, name).toContain('run-local-playwright.mjs')
    }
  })
})
