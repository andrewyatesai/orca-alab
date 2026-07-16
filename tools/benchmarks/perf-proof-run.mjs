#!/usr/bin/env node
/**
 * Perf-proof lane runner (`pnpm bench:perf`): captures every headline perf
 * claim in ONE structured run file so bench:check can gate later changes
 * against the committed trend (tools/benchmarks/trends/).
 *
 * What it drives, headlessly:
 *   1. tests/e2e/aterm-perf-proof.spec.ts — a real Electron session measuring
 *      typed keydown→frame-presented (User Timing marks from
 *      aterm-pane-present.ts) plus the in-renderer __atermLatencyBench /
 *      __atermGpuCpuBench / __atermMemoryBench harnesses.
 *   2. tools/benchmarks/startup-time-bench.mjs — spawn → did-finish-load.
 *   3. Optional --engine-log <file>: folds raw criterion output from the
 *      manual aterm engine step (see docs/reference/perf-proof.md).
 *
 * Usage:
 *   pnpm bench:perf [-- --label mylabel --skip-build --engine-log <file> ...]
 *
 * Output: tools/benchmarks/results/perf-proof-<label>-<stamp>.json
 * Then:   pnpm bench:check           — gate vs the committed trend
 *         pnpm bench:check -- --accept  — append this run to the trend
 */
import { spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import { join, resolve } from 'node:path'
import { parseCriterionOutput } from './criterion-output-parse.mjs'
import { extractMetrics, formatMetricValue, PERF_PROOF_METRICS } from './perf-proof-metrics.mjs'

const scriptDir = import.meta.dirname
const repoRoot = resolve(scriptDir, '..', '..')
const resultsDir = join(scriptDir, 'results')

function parseArgs(argv) {
  const args = {
    label: 'local',
    skipE2e: false,
    skipStartup: false,
    skipBuild: false,
    startupIterations: 3,
    startupFiles: 2000,
    engineLogs: []
  }
  for (let i = 2; i < argv.length; i++) {
    const next = () => argv[++i]
    switch (argv[i]) {
      // pnpm forwards the `--` separator literally.
      case '--':
        break
      case '--label':
        args.label = next()
        break
      case '--skip-e2e':
        args.skipE2e = true
        break
      case '--skip-startup':
        args.skipStartup = true
        break
      case '--skip-build':
        args.skipBuild = true
        break
      case '--startup-iterations':
        args.startupIterations = Number(next())
        break
      case '--startup-files':
        args.startupFiles = Number(next())
        break
      case '--engine-log':
        args.engineLogs.push(next())
        break
      default:
        throw new Error(`Unknown argument: ${argv[i]}`)
    }
  }
  return args
}

// npx/node resolution needs a shell on Windows (npx is npx.cmd there).
const needsShell = process.platform === 'win32'

function runE2eLane({ skipBuild }) {
  const rawOut = join(os.tmpdir(), `orca-perf-proof-raw-${process.pid}-${Date.now()}.json`)
  console.log('[perf-proof] driving the in-app harnesses via Playwright (electron-headless)…')
  const result = spawnSync(
    'npx',
    [
      'playwright',
      'test',
      'tests/e2e/aterm-perf-proof.spec.ts',
      '--config',
      'tests/playwright.config.ts',
      '--project',
      'electron-headless',
      '--workers=1'
    ],
    {
      cwd: repoRoot,
      stdio: 'inherit',
      shell: needsShell,
      env: {
        ...process.env,
        ORCA_PERF_PROOF: '1',
        ORCA_PERF_PROOF_OUT: rawOut,
        ...(skipBuild ? { SKIP_BUILD: '1' } : {})
      },
      // The spec builds several throwaway engines (CPU + GPU at several grids)
      // on top of the app build itself.
      timeout: 20 * 60 * 1000
    }
  )
  if (result.status !== 0 || !existsSync(rawOut)) {
    throw new Error(`perf-proof e2e lane failed (exit ${result.status}); no raw run at ${rawOut}`)
  }
  const raw = JSON.parse(readFileSync(rawOut, 'utf-8'))
  rmSync(rawOut, { force: true })
  return raw
}

function runStartupBench({ iterations, files }) {
  console.log(`[perf-proof] startup bench (${iterations} iterations, ${files}-file fixture)…`)
  const result = spawnSync(
    process.execPath,
    [
      join(scriptDir, 'startup-time-bench.mjs'),
      '--label',
      'perf-proof',
      '--iterations',
      String(iterations),
      '--files',
      String(files)
    ],
    {
      cwd: repoRoot,
      encoding: 'utf-8',
      stdio: ['ignore', 'pipe', 'inherit'],
      timeout: 15 * 60 * 1000
    }
  )
  process.stdout.write(result.stdout ?? '')
  if (result.status !== 0) {
    throw new Error(`startup bench failed (exit ${result.status})`)
  }
  const match = /\[bench\] results written to (.+)/.exec(result.stdout ?? '')
  if (!match) {
    throw new Error('startup bench did not report a results path')
  }
  return JSON.parse(readFileSync(match[1].trim(), 'utf-8'))
}

function foldEngineLogs(paths) {
  const engine = {}
  for (const logPath of paths) {
    const parsed = parseCriterionOutput(readFileSync(resolve(logPath), 'utf-8'))
    const found = Object.keys(parsed).length
    console.log(`[perf-proof] folded ${found} criterion benches from ${logPath}`)
    Object.assign(engine, parsed)
  }
  return Object.keys(engine).length > 0 ? engine : null
}

function gitCommit() {
  const result = spawnSync('git', ['rev-parse', '--short', 'HEAD'], {
    cwd: repoRoot,
    encoding: 'utf-8'
  })
  if (result.status !== 0) {
    return null
  }
  // Mark runs from a dirty tree so a trend entry can't masquerade as a
  // measurement of the named commit.
  const dirty = spawnSync('git', ['status', '--porcelain'], { cwd: repoRoot, encoding: 'utf-8' })
  const suffix = dirty.status === 0 && dirty.stdout.trim().length > 0 ? '-dirty' : ''
  return `${result.stdout.trim()}${suffix}`
}

async function main() {
  const args = parseArgs(process.argv)

  if (args.skipE2e && !existsSync(join(repoRoot, 'out', 'main', 'index.js'))) {
    throw new Error('--skip-e2e needs an existing build — run `pnpm build:electron-vite` first')
  }

  const e2e = args.skipE2e ? null : runE2eLane({ skipBuild: args.skipBuild })
  const startup = args.skipStartup
    ? null
    : runStartupBench({ iterations: args.startupIterations, files: args.startupFiles })
  const engine = foldEngineLogs(args.engineLogs)

  const run = {
    schema: 1,
    label: args.label,
    capturedAt: new Date().toISOString(),
    commit: gitCommit(),
    platform: process.platform,
    arch: process.arch,
    cpus: os.cpus()[0]?.model ?? null,
    keydown: e2e?.keydown ?? null,
    latency: e2e?.latency ?? null,
    gpuCpu: e2e?.gpuCpu ?? null,
    memory: e2e?.memory ?? null,
    startup,
    engine
  }

  mkdirSync(resultsDir, { recursive: true })
  const stamp = new Date().toISOString().replace(/[:.]/g, '-')
  const outPath = join(resultsDir, `perf-proof-${args.label}-${stamp}.json`)
  writeFileSync(outPath, `${JSON.stringify(run, null, 2)}\n`)

  const metrics = extractMetrics(run)
  console.log(`\n[perf-proof] run ${args.label} @ ${run.commit ?? '<no-commit>'}`)
  console.log('| metric | value |')
  console.log('|---|---|')
  for (const metric of PERF_PROOF_METRICS) {
    console.log(`| ${metric.label} | ${formatMetricValue(metrics[metric.id], metric.unit)} |`)
  }
  console.log(`\n[perf-proof] full run written to ${outPath}`)
  console.log('[perf-proof] next: `pnpm bench:check` (gate) or `pnpm bench:check -- --accept`')
}

main().catch((error) => {
  console.error(error)
  process.exit(1)
})
