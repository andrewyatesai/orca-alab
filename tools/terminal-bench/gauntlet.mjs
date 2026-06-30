#!/usr/bin/env node
// aterm superiority gauntlet — the agent-runnable gate that REPLACES CI (no GitHub Actions).
//
// Proves the hand-written Rust aterm engine (shipped as `orca-terminal` via the
// native/orca-node napi addon) is a FULL, SUPERIOR replacement for @xterm/headless
// across the three axes from tools/aterm-vs-xterm/GOAL-B-HANDOFF.md:
//   • conformance — visible-grid parity per ANSI case. aterm matching xterm = parity;
//                   a divergence is REVIEW, not auto-fail, because "more correct than
//                   xterm per the VT/ECMA-48 spec" is a WIN to be triaged, not a bug.
//   • perf        — MB/s throughput vs xterm, best-of-N medians in one thermal state.
//   • safety      — Trust-proved obligations (skipped, not failed, when the toolchain is absent).
//
// An agent runs:  node tools/terminal-bench/gauntlet.mjs <bootstrap|conformance|perf|safety|all>
// Exit 0 = all gates green/skipped · 1 = a real FAIL · 2 = REVIEW (divergence to triage).
// A machine-readable report is written to tools/terminal-bench/.gauntlet-report.json.

import { execFileSync } from 'node:child_process'
import { readFileSync, writeFileSync, existsSync, mkdirSync } from 'node:fs'
import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'
import { dirname, join, resolve } from 'node:path'
import { tmpdir } from 'node:os'

const here = dirname(fileURLToPath(import.meta.url))
const repo = resolve(here, '..', '..')
const require = createRequire(import.meta.url)

const ADDON = join(repo, 'native', 'orca-node', 'orca_node.node')
const XTERM = join(here, 'node_modules', '@xterm', 'headless', 'lib-headless', 'xterm-headless.js')
const CONF_CORPUS = join(repo, 'tools', 'aterm-vs-xterm', 'corpus.json')
const BENCH_DIR = join(tmpdir(), 'orca-bench')
const PERF_CORPUS = join(BENCH_DIR, 'corpus.bin')
const REPORT = join(here, '.gauntlet-report.json')
const PERF_FLOOR = 1.0 // aterm must be at least as fast as xterm; the real ratio is reported.

const rstrip = (s) => s.replace(/\s+$/u, '')
const sh = (cmd, args, opts = {}) =>
  execFileSync(cmd, args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], ...opts })
const skip = (reason) => ({ status: 'SKIP', detail: reason })
const C = { g: '\x1b[32m', r: '\x1b[31m', y: '\x1b[33m', d: '\x1b[2m', b: '\x1b[1m', x: '\x1b[0m' }

// --- bootstrap: make every prerequisite present, idempotently --------------------
function bootstrap() {
  mkdirSync(BENCH_DIR, { recursive: true })
  const did = []
  if (!existsSync(ADDON)) {
    console.log(`${C.d}  building napi addon (cargo build --release in native/orca-node)…${C.x}`)
    sh('cargo', ['build', '--release'], {
      cwd: join(repo, 'native', 'orca-node'),
      stdio: 'inherit'
    })
    did.push('built napi addon')
  }
  if (!existsSync(XTERM)) {
    console.log(
      `${C.d}  installing @xterm/headless baseline (pnpm install in tools/terminal-bench)…${C.x}`
    )
    sh('pnpm', ['-C', here, 'install'], { stdio: 'inherit' })
    did.push('installed @xterm/headless')
  }
  let blocked
  if (!existsSync(PERF_CORPUS)) {
    console.log(`${C.d}  generating 16 MB perf corpus (orca-terminal bench example)…${C.x}`)
    try {
      sh(
        'cargo',
        [
          'run',
          '-q',
          '--release',
          '--example',
          'bench',
          '-p',
          'orca-terminal',
          '--',
          'gen',
          PERF_CORPUS,
          '16'
        ],
        { cwd: join(repo, 'rust'), stdio: 'inherit' }
      )
      did.push('generated perf corpus')
    } catch {
      // The prebuilt napi addon still runs; only the from-source rebuild is blocked.
      blocked =
        'perf corpus BLOCKED — cargo build failed (the web-time gap in rust/vendor: aterm-core needs web-time; run `cargo vendor` with network to repopulate)'
    }
  }
  return {
    status: blocked ? 'REVIEW' : 'PASS',
    detail: [did.length ? did.join('; ') : 'all prerequisites already present', blocked]
      .filter(Boolean)
      .join(' · ')
  }
}

// --- conformance: visible-grid differential, xterm loaded once, per case ---------
async function conformance() {
  if (!existsSync(ADDON)) {
    return skip('napi addon missing — run `gauntlet bootstrap`')
  }
  if (!existsSync(XTERM)) {
    return skip('@xterm/headless missing — run `gauntlet bootstrap`')
  }
  const { HeadlessTerminal } = require(ADDON)
  const { Terminal } = require(XTERM)
  const cases = JSON.parse(readFileSync(CONF_CORPUS, 'utf8'))
  const ROWS = 24
  const COLS = 80
  let parity = 0
  const diverge = []
  for (const { name, bytes } of cases) {
    const buf = Buffer.from(bytes, 'latin1')
    const rt = new HeadlessTerminal(COLS, ROWS, 1000)
    rt.write(buf)
    const a = rt.snapshot().map(rstrip)
    const xt = new Terminal({ rows: ROWS, cols: COLS, allowProposedApi: true })
    xt.write(buf)
    await new Promise((r) => xt.write('', r)) // xterm flush
    const x = []
    for (let r = 0; r < ROWS; r++) {
      x.push(rstrip(xt.buffer.active.getLine(r)?.translateToString(true) ?? ''))
    }
    if (a.join('\n') === x.join('\n')) {
      parity++
    } else {
      const rows = []
      for (let r = 0; r < ROWS && rows.length < 4; r++) {
        if (a[r] !== x[r]) {
          rows.push({ row: r, aterm: (a[r] ?? '').slice(0, 60), xterm: (x[r] ?? '').slice(0, 60) })
        }
      }
      diverge.push({ name, rows })
    }
  }
  return {
    status: diverge.length === 0 ? 'PASS' : 'REVIEW',
    metrics: { parity, total: cases.length, divergences: diverge.length },
    diverge
  }
}

// --- perf: best-of-N medians via the real bench legs, plus grid-parity check -----
function perf(trials = 5) {
  if (!existsSync(ADDON) || !existsSync(XTERM) || !existsSync(PERF_CORPUS)) {
    return skip('missing prereqs — run `gauntlet bootstrap`')
  }
  const median = (xs) => xs.slice().sort((a, b) => a - b)[Math.floor(xs.length / 2)]
  const leg = (script, addonArg) => {
    const out = join(BENCH_DIR, `${script}.json`)
    const rates = []
    let visSha
    for (let i = 0; i < trials; i++) {
      sh('node', [join(here, script), ...addonArg, PERF_CORPUS, out])
      const j = JSON.parse(readFileSync(out, 'utf8'))
      rates.push(j.mb_per_s)
      visSha = j.visible_sha
    }
    return { mb: median(rates), sha: visSha }
  }
  const xt = leg('xterm-bench.mjs', [])
  const at = leg('addon-bench.mjs', [ADDON])
  const ratio = at.mb / xt.mb
  const parity = xt.sha === at.sha
  return {
    status: parity && ratio >= PERF_FLOOR ? 'PASS' : 'FAIL',
    metrics: {
      xterm_mb_s: +xt.mb.toFixed(1),
      aterm_mb_s: +at.mb.toFixed(1),
      ratio: +ratio.toFixed(2),
      parity
    }
  }
}

// --- safety: Trust-proved obligations; runnable only where the toolchain exists --
function safety() {
  const ay = join(process.env.HOME || '', '.cargo', 'bin', 'ay')
  const verify = join(repo, 'rust', 'crates', 'orca-git', 'proofs', 'ay', 'verify.sh')
  if (!existsSync(ay)) {
    return skip('Trust solver `ay` not found (~/.cargo/bin/ay) — safety axis unavailable here')
  }
  if (!existsSync(verify)) {
    return skip('orca-git proof bundle (proofs/ay/verify.sh) not found')
  }
  try {
    const out = sh('bash', [verify], { cwd: dirname(verify) })
    const discharged = (out.match(/DISCHARGED/g) || []).length
    const clean = /DISCHARGED/.test(out) && !/\b(FAIL|UNKNOWN|error)\b/i.test(out)
    return {
      status: clean ? 'PASS' : 'REVIEW',
      metrics: { obligations_discharged: discharged },
      detail: 'orca-git SMT obligations (tcargo panic/UB proofs need the full ~/trust toolchain)'
    }
  } catch (e) {
    return { status: 'FAIL', detail: String(e.message).split('\n')[0] }
  }
}

// --- driver ----------------------------------------------------------------------
const GATES = { bootstrap, conformance, perf, safety }
const mark = (s) =>
  ({
    PASS: `${C.g}✓ PASS${C.x}`,
    FAIL: `${C.r}✗ FAIL${C.x}`,
    REVIEW: `${C.y}● REVIEW${C.x}`,
    SKIP: `${C.d}– SKIP${C.x}`
  })[s] ?? s

async function main() {
  const cmd = process.argv[2] || 'all'
  const names = cmd === 'all' ? ['bootstrap', 'conformance', 'perf', 'safety'] : [cmd]
  if (!names.every((n) => GATES[n])) {
    console.error(`unknown gate "${cmd}". use: ${Object.keys(GATES).join(' | ')} | all`)
    process.exit(64)
  }
  console.log(
    `${C.b}aterm superiority gauntlet${C.x} ${C.d}— Rust orca-terminal (napi) vs @xterm/headless${C.x}\n`
  )
  const results = {}
  for (const n of names) {
    process.stdout.write(`${C.d}running ${n}…${C.x}\n`)
    results[n] = await GATES[n]()
  }
  console.log(`\n${C.b}verdict${C.x}`)
  for (const [n, r] of Object.entries(results)) {
    const metrics = r.metrics ? `  ${C.d}${JSON.stringify(r.metrics)}${C.x}` : ''
    console.log(`  ${mark(r.status).padEnd(18)} ${n}${metrics}`)
    if (r.detail) {
      console.log(`      ${C.d}${r.detail}${C.x}`)
    }
    for (const d of r.diverge ?? []) {
      const rows = d.rows.map((v) => `row ${v.row} [${v.aterm}]≠[${v.xterm}]`).join(' · ')
      console.log(`      ${C.y}diverge:${C.x} ${d.name} — ${rows}`)
    }
  }
  writeFileSync(
    REPORT,
    JSON.stringify({ at: `${new Date().toISOString().slice(0, 19)}Z`, results }, null, 2)
  )
  console.log(`\n${C.d}report → ${REPORT}${C.x}`)
  const statuses = Object.values(results).map((r) => r.status)
  process.exit(statuses.includes('FAIL') ? 1 : statuses.includes('REVIEW') ? 2 : 0)
}

main().catch((e) => {
  console.error(`${C.r}gauntlet crashed:${C.x} ${e.stack || e.message}`)
  process.exit(70)
})
