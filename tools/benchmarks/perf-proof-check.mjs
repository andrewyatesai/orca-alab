#!/usr/bin/env node
/**
 * Perf-proof regression gate (`pnpm bench:check`): compares the newest
 * perf-proof run against the LAST entry of this machine's committed trend
 * (tools/benchmarks/trends/<platform>-<arch>.json) and fails on a >15%
 * regression on any gated metric (see perf-proof-metrics.mjs for the catalog
 * and per-metric policy).
 *
 * Usage:
 *   pnpm bench:check                    — gate the newest run
 *   pnpm bench:check -- --accept        — gate, then append the run to the trend
 *                                         (commit the trend file to publish it)
 *   pnpm bench:check -- --run <file>    — gate a specific run file
 *   pnpm bench:check -- --trend <file>  — compare against a specific trend file
 */
import { existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { comparePerfMetrics } from './perf-proof-compare.mjs'
import {
  extractMetrics,
  formatMetricValue,
  machineKeyOf,
  REGRESSION_THRESHOLD
} from './perf-proof-metrics.mjs'

const scriptDir = import.meta.dirname
const resultsDir = join(scriptDir, 'results')
const trendsDir = join(scriptDir, 'trends')

/** Trend history cap: enough to see quarters of drift without unbounded growth. */
const MAX_TREND_RUNS = 60

function parseArgs(argv) {
  const args = { runPath: null, trendPath: null, accept: false }
  for (let i = 2; i < argv.length; i++) {
    const next = () => argv[++i]
    switch (argv[i]) {
      // pnpm forwards the `--` separator literally.
      case '--':
        break
      case '--run':
        args.runPath = resolve(next())
        break
      case '--trend':
        args.trendPath = resolve(next())
        break
      case '--accept':
        args.accept = true
        break
      default:
        throw new Error(`Unknown argument: ${argv[i]}`)
    }
  }
  return args
}

function newestRunPath() {
  if (!existsSync(resultsDir)) {
    return null
  }
  const candidates = readdirSync(resultsDir)
    .filter((name) => name.startsWith('perf-proof-') && name.endsWith('.json'))
    .map((name) => join(resultsDir, name))
    .sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs)
  return candidates[0] ?? null
}

function formatDelta(deltaPct) {
  if (deltaPct === null || deltaPct === undefined) {
    return ''
  }
  const pct = (deltaPct * 100).toFixed(1)
  if (Number(pct) === 0) {
    return 'no change'
  }
  // Positive is normalized to "worse" by the compare; show it with its sign.
  return deltaPct > 0 ? `+${pct}% worse` : `${Math.abs(Number(pct))}% better`
}

const STATUS_TAGS = {
  ok: 'OK  ',
  regressed: 'FAIL',
  lost: 'LOST',
  new: 'NEW ',
  skipped: 'SKIP',
  info: 'INFO'
}

function printComparison(results) {
  for (const row of results) {
    const tag = STATUS_TAGS[row.status] ?? row.status
    const baseline = formatMetricValue(row.baseline, row.unit)
    const latest = formatMetricValue(row.latest, row.unit)
    const delta = formatDelta(row.deltaPct)
    console.log(`  [${tag}] ${row.label}: ${baseline} -> ${latest}${delta ? ` (${delta})` : ''}`)
  }
}

function main() {
  const args = parseArgs(process.argv)

  const runPath = args.runPath ?? newestRunPath()
  if (!runPath || !existsSync(runPath)) {
    throw new Error('no perf-proof run found — run `pnpm bench:perf` first')
  }
  const run = JSON.parse(readFileSync(runPath, 'utf-8'))
  const machineKey = machineKeyOf(run)
  const metrics = extractMetrics(run)
  const trendPath = args.trendPath ?? join(trendsDir, `${machineKey}.json`)

  console.log(`[bench:check] run:   ${runPath}`)
  console.log(`[bench:check] trend: ${trendPath} (machine ${machineKey})`)

  const entry = {
    capturedAt: run.capturedAt,
    label: run.label,
    commit: run.commit ?? null,
    metrics
  }

  if (!existsSync(trendPath)) {
    if (!args.accept) {
      console.error(
        `[bench:check] no committed trend for ${machineKey}. Seed one with:\n` +
          '  pnpm bench:check -- --accept\nthen commit the trend file.'
      )
      process.exit(1)
    }
    mkdirSync(trendsDir, { recursive: true })
    const trend = {
      schema: 1,
      machine: { key: machineKey, platform: run.platform, arch: run.arch, cpus: run.cpus },
      thresholdPct: REGRESSION_THRESHOLD * 100,
      runs: [entry]
    }
    writeFileSync(trendPath, `${JSON.stringify(trend, null, 2)}\n`)
    console.log(`[bench:check] seeded new trend at ${trendPath} — commit it to publish.`)
    return
  }

  const trend = JSON.parse(readFileSync(trendPath, 'utf-8'))
  const baseline = trend.runs.at(-1)
  if (!baseline) {
    throw new Error(`trend file ${trendPath} has no runs`)
  }
  console.log(
    `[bench:check] baseline: ${baseline.label} @ ${baseline.commit ?? '<no-commit>'} ` +
      `(${baseline.capturedAt})`
  )

  const { results, failures } = comparePerfMetrics(baseline.metrics, metrics)
  printComparison(results)

  if (failures.length > 0) {
    console.error(
      `\n[bench:check] FAIL — ${failures.length} metric(s) regressed beyond ` +
        `${REGRESSION_THRESHOLD * 100}% vs the committed trend:`
    )
    for (const failure of failures) {
      const lostNote = failure.status === 'lost' ? ' (metric lost — dead harness or lost path)' : ''
      console.error(
        `  - ${failure.label}: ${formatMetricValue(failure.baseline, failure.unit)} -> ${formatMetricValue(failure.latest, failure.unit)}${lostNote}`
      )
    }
    console.error(
      '[bench:check] fix the regression, or (for an intentional trade-off) re-run and ' +
        'append the new baseline with `pnpm bench:check -- --accept` in the same change.'
    )
    process.exit(1)
  }

  console.log(
    `\n[bench:check] PASS — no gated metric regressed beyond ${REGRESSION_THRESHOLD * 100}%.`
  )

  if (args.accept) {
    trend.runs.push(entry)
    if (trend.runs.length > MAX_TREND_RUNS) {
      trend.runs = trend.runs.slice(-MAX_TREND_RUNS)
    }
    writeFileSync(trendPath, `${JSON.stringify(trend, null, 2)}\n`)
    console.log(`[bench:check] appended run to ${trendPath} — commit it to publish.`)
  }
}

try {
  main()
} catch (error) {
  console.error(error)
  process.exit(1)
}
