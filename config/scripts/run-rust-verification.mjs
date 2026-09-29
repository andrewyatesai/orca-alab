#!/usr/bin/env node
// Run Trust verification over the first-party crates and report what was proved.
//
// WHY THIS EXISTS SEPARATELY FROM EVERY OTHER BUILD. Every routine script builds
// with the Trust toolchain's fast lane (`targo --unverified`), which makes no
// proof claim. This is the lane where verification is the point:
// `targo trust check -p <crate> --allow-l0-gaps --format json`, run from rust/.
// It REPORTS rather than gates: verification is slow, and a gate everyone learns
// to skip is worse than a report someone reads.
//
// USAGE
//   node config/scripts/run-rust-verification.mjs                  # default set
//   node config/scripts/run-rust-verification.mjs orca-core        # named crates
//   node config/scripts/run-rust-verification.mjs --all            # every orca-* crate
//   node config/scripts/run-rust-verification.mjs --json           # machine output
//
// EXIT: 0 when every requested crate was MEASURED (targo emitted its JSON
// report), whatever the verdicts — an unproved obligation is information, and
// `--allow-l0-gaps` is the advisory survey policy that makes that true. Non-zero
// only when a crate produced no report (it failed to compile, or verification
// did not run) or targo is missing, because both mean the run measured nothing.
// targo trust refuses a world-writable CARGO_TARGET_DIR (e.g. under /tmp).
import { spawnSync } from 'node:child_process'
import { readdirSync } from 'node:fs'
import { join } from 'node:path'
import { RustToolchainError, resolveTargo } from './rust-toolchain-lane.mjs'

const ROOT = new URL('../..', import.meta.url).pathname
const RUST = join(ROOT, 'rust')

const argv = process.argv.slice(2)
const wantJson = argv.includes('--json')
const wantAll = argv.includes('--all')
const named = argv.filter((a) => !a.startsWith('--'))

// The cores that carry ported logic and vectors. Not the whole workspace by
// default: orca-aterm-demo/orca-parity are harnesses, not shipped logic.
const DEFAULT_CRATES = [
  'orca-core',
  'orca-config',
  'orca-git',
  'orca-text',
  'orca-policy',
  'orca-agents'
]

function allCrates() {
  return readdirSync(join(RUST, 'crates'))
    .filter((c) => c.startsWith('orca-'))
    .sort()
}

const crates = named.length > 0 ? named : wantAll ? allCrates() : DEFAULT_CRATES

let targo
try {
  targo = resolveTargo().command
} catch (error) {
  if (!(error instanceof RustToolchainError)) {
    throw error
  }
  console.error(`[verify:rust] ${error.message}`)
  console.error('              Nothing was verified. This is a hard failure, not a skip: a run')
  console.error('              that measures nothing must not look like a run that found nothing.')
  process.exit(1)
}

/** The tally from targo's JSON report, or null when no report was emitted. */
function tally(stdout) {
  let report
  try {
    report = JSON.parse(stdout)
  } catch {
    return null
  }
  const s = report?.summary
  if (!s || typeof s.total_obligations !== 'number') {
    return null
  }
  return {
    functions: s.functions_analyzed ?? 0,
    obligations: s.total_obligations,
    proved: s.total_proved ?? 0,
    failed: s.total_failed ?? 0,
    unknown: s.total_unknown ?? 0,
    timedOut: s.total_timed_out ?? 0,
    runtimeChecked: s.total_runtime_checked ?? 0,
    gate: report.verification_gate?.decision ?? 'unknown'
  }
}

const results = []
for (const crate of crates) {
  const started = process.hrtime.bigint()
  process.stderr.write(`[verify:rust] ${crate} … `)
  const run = spawnSync(
    targo,
    ['trust', 'check', '-p', crate, '--allow-l0-gaps', '--format', 'json'],
    { cwd: RUST, encoding: 'utf8', maxBuffer: 512e6 }
  )
  const seconds = Number(process.hrtime.bigint() - started) / 1e9
  const t = tally(run.stdout ?? '')
  const compiled = t !== null
  results.push({ crate, compiled, seconds, ...t })
  process.stderr.write(
    compiled
      ? `ok ${seconds.toFixed(0)}s — ${t.obligations} obligation(s), ${t.proved} proved, gate ${t.gate}\n`
      : `NOT MEASURED (${seconds.toFixed(0)}s, exit ${run.status})\n`
  )
  if (!compiled) {
    const first =
      (run.stderr ?? '').split('\n').find((l) => /^(targo trust: )?error/.test(l)) ??
      '(no error line)'
    process.stderr.write(`               ${first}\n`)
  }
}

if (wantJson) {
  console.log(JSON.stringify({ crates: results }, null, 2))
} else {
  const pad = (s, n) => String(s).padEnd(n)
  const num = (s, n) => String(s).padStart(n)
  console.log('')
  console.log(
    `  ${pad('crate', 22)}${num('oblig', 7)}${num('proved', 8)}${num('failed', 8)}${num('unknown', 9)}${num('timeout', 9)}${num('rt-chk', 8)}${num('secs', 7)}`
  )
  console.log(`  ${'-'.repeat(78)}`)
  for (const r of results) {
    if (!r.compiled) {
      console.log(`  ${pad(r.crate, 22)}${num('NOT MEASURED', 47)}${num(r.seconds.toFixed(0), 7)}`)
      continue
    }
    console.log(
      `  ${pad(r.crate, 22)}${num(r.obligations, 7)}${num(r.proved, 8)}${num(r.failed, 8)}${num(r.unknown, 9)}${num(r.timedOut, 9)}${num(r.runtimeChecked, 8)}${num(r.seconds.toFixed(0), 7)}`
    )
  }
  const sum = (k) => results.filter((r) => r.compiled).reduce((n, r) => n + r[k], 0)
  console.log(`  ${'-'.repeat(78)}`)
  console.log(
    `  ${pad('total', 22)}${num(sum('obligations'), 7)}${num(sum('proved'), 8)}${num(sum('failed'), 8)}${num(sum('unknown'), 9)}${num(sum('timedOut'), 9)}${num(sum('runtimeChecked'), 8)}${num(sum('seconds').toFixed(0), 7)}`
  )
  console.log('')
  if (sum('timedOut') > 0) {
    console.log(`  ${sum('timedOut')} obligation(s) TIMED OUT. A timeout is an assumption, not a`)
    console.log('  proof, and which functions hit it can vary with machine load.')
    console.log('')
  }
  console.log('  Unproved is not the same as wrong, and proved is not the whole story: unknown,')
  console.log('  timed-out and runtime-checked rows are assumptions. Read the per-crate report')
  console.log('  (`targo trust check -p <crate>` from rust/) before quoting a verdict.')
  console.log('')
}

const brokeCompile = results.filter((r) => !r.compiled)
if (brokeCompile.length > 0) {
  console.error(
    `[verify:rust] ${brokeCompile.length} crate(s) produced no report: ${brokeCompile.map((r) => r.crate).join(', ')}`
  )
  process.exit(1)
}
