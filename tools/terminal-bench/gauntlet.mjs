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
//   • autoformalize — Goal A: reuse the Trust ts2rust two-witness gate (~/trust/tools/ts2rust)
//                   to prove the orc corpus's Rust ports refine their TS (skipped if trustc absent).
//
// An agent runs:  node tools/terminal-bench/gauntlet.mjs <bootstrap|conformance|perf|safety|autoformalize|all>
// Exit 0 = every gate green · 1 = a real FAIL · 2 = REVIEW (divergence to triage).
// For `all`, a SKIP is NOT a pass: any skipped gate exits 2 so an environment that
// can't run an axis never reads as green. A single-gate invocation may exit 0 on
// SKIP (so probing one axis stays scriptable) but says so loudly.
// A machine-readable report is written to tools/terminal-bench/.gauntlet-report.json.

import { execFileSync } from 'node:child_process'
import { readFileSync, writeFileSync, existsSync, mkdirSync, readdirSync } from 'node:fs'
import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'
import { dirname, join, resolve } from 'node:path'
import { tmpdir } from 'node:os'
import { loadCorpus } from '../aterm-vs-xterm/corpus-bytes.mjs'

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
// rustup-stable pin for direct cargo invocations (perf corpus): the machine default
// toolchain may be a nightly older than the workspace's rust-version, and a
// Homebrew cargo shadowing rustup ignores rust-toolchain.toml.
const rustupStable = (tool) => {
  try {
    return sh('rustup', ['which', tool, '--toolchain', 'stable']).trim()
  } catch {
    return null
  }
}

function bootstrap() {
  mkdirSync(BENCH_DIR, { recursive: true })
  const did = []
  if (!existsSync(ADDON)) {
    // The build script owns the cdylib→orca_node.node rename, submodule init and
    // toolchain pinning — a raw `cargo build` here would leave ADDON missing.
    console.log(`${C.d}  building napi addon (config/scripts/build-terminal-addon.mjs)…${C.x}`)
    sh('node', [join(repo, 'config', 'scripts', 'build-terminal-addon.mjs'), '--if-missing'], {
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
      // Invoke from the repo ROOT (cargo reads .cargo/config from the cwd, so this
      // escapes rust/'s offline-vendor replacement and resolves online), with the
      // rustup-stable pin — same recipe as run-parity.mjs / build-aterm-wasm.mjs.
      const cargo = rustupStable('cargo')
      const rustc = rustupStable('rustc')
      sh(
        cargo ?? 'cargo',
        [
          'run',
          '-q',
          '--release',
          '--example',
          'bench',
          '-p',
          'orca-terminal',
          '--manifest-path',
          'rust/Cargo.toml',
          '--',
          'gen',
          PERF_CORPUS,
          '16'
        ],
        {
          cwd: repo,
          stdio: 'inherit',
          env: { ...process.env, CARGO_NET_OFFLINE: 'false', ...(rustc ? { RUSTC: rustc } : {}) }
        }
      )
      did.push('generated perf corpus')
    } catch {
      // The prebuilt napi addon still runs; only the from-source rebuild is blocked.
      blocked =
        'perf corpus BLOCKED — cargo build failed (see output above; rust/vendor carries the full lockfile closure, so this should build offline)'
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
  const cases = loadCorpus(CONF_CORPUS)
  const ROWS = 24
  const COLS = 80
  let parity = 0
  const diverge = []
  for (const { name, bytes: buf, comment } of cases) {
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
      // A case may pre-document its expected divergence (e.g. invalid UTF-8 where
      // aterm is the more-correct engine) — surface it for the REVIEW triage.
      diverge.push(comment ? { name, comment, rows } : { name, rows })
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

// --- autoformalize: the Trust ts2rust two-witness gate over the orc corpus -------
// Goal A. Reuses the EXISTING autoformalizer in the Trust repo (~/trust/tools/ts2rust):
// it discovers the already-ported .ts/.rs pairs, derives each fn + argspec straight
// from the candidate's signature, and runs W1 (trustc ∀-safety) + W2 (Node-TS diff).
// SKIPs (never fakes) when the Trust harness or the trustc toolchain isn't present.
const TS2RUST = join(process.env.HOME || '', 'trust', 'tools', 'ts2rust')
const PRIM = new Set(['u32', 'i32', 'u64', 'i64', 'bool'])
const SLICE = {
  '&[u32]': 'u32[]',
  '&[i32]': 'i32[]',
  '&[u64]': 'u64[]',
  '&[i64]': 'i64[]',
  '&[&str]': 'str[]'
}

function locateTrustc() {
  const candidates = [
    process.env.TRUSTC,
    join(process.env.HOME || '', 'trust', 'build', 'host', 'stage2', 'bin', 'trustc')
  ]
  for (const c of candidates) {
    if (c && existsSync(c)) {
      return c
    }
  }
  try {
    return sh('bash', ['-lc', 'command -v trustc']).trim() || null
  } catch {
    return null
  }
}

function rustTypeToArgspec(ty, src) {
  const t = ty.replace(/\s+/gu, '')
  if (PRIM.has(t)) {
    return t
  }
  if (t === '&str') {
    return 'str'
  }
  if (SLICE[t]) {
    return SLICE[t]
  }
  if (/^[A-Z]\w*$/u.test(t)) {
    const m = src.match(new RegExp(`struct\\s+${t}\\s*\\{([^}]*)\\}`, 'u'))
    const fields = m ? [...m[1].matchAll(/(?:pub\s+)?(\w+)\s*:/gu)].map((x) => x[1]) : []
    return fields.length ? `${t}{${fields.join(',')}}` : null
  }
  return null
}

function discoverCorpus(orcaDir) {
  const out = []
  let files
  try {
    files = readdirSync(orcaDir).filter((f) => f.endsWith('.rs'))
  } catch {
    return out
  }
  for (const rs of files) {
    const name = rs.slice(0, -3)
    if (!existsSync(join(orcaDir, `${name}.ts`))) {
      continue // the driver needs a same-named .ts reference kernel
    }
    const src = readFileSync(join(orcaDir, rs), 'utf8')
    const sig = src.match(/pub\s+fn\s+(\w+)\s*\(([^)]*)\)/u)
    if (!sig) {
      continue
    }
    const params = sig[2].trim()
    const specs = []
    let ok = true
    for (const p of params ? params.split(',') : []) {
      const a = rustTypeToArgspec(p.split(':').slice(1).join(':'), src)
      if (!a) {
        ok = false
        break
      }
      specs.push(a)
    }
    if (!ok) {
      out.push({ name, fn: sig[1], declined: true })
    } else {
      // Convention: a deliberately-buggy port is named *_bug / *_naive (suffix), expected to be refuted.
      // (Don't match substrings — e.g. `..._toobig` is a real predicate name, a faithful port.)
      out.push({
        name,
        fn: sig[1],
        argspec: specs.join(','),
        expect: /_(bug|naive)$/u.test(name) ? 'NOT-TRUSTED' : 'TRUSTED'
      })
    }
  }
  return out
}

function autoformalize() {
  const driver = join(TS2RUST, 'autoformalize.mjs')
  if (!existsSync(driver)) {
    return skip(
      'Trust ts2rust harness not found (~/trust/tools/ts2rust) — Goal A engine lives in the Trust repo'
    )
  }
  const corpus = discoverCorpus(join(TS2RUST, 'orca'))
  const runnable = corpus.filter((c) => !c.declined)
  if (!runnable.length) {
    return skip('no autoformalizable .ts/.rs pairs discovered under ~/trust/tools/ts2rust/orca')
  }
  const trustc = locateTrustc()
  if (!trustc) {
    return {
      status: 'SKIP',
      metrics: { corpus: runnable.length, declined: corpus.length - runnable.length },
      detail: `trustc not built — ${runnable.length} orc functions ready to autoformalize; build ~/trust (stage2) or set TRUSTC=<path>, then re-run`
    }
  }
  const rows = []
  for (const c of runnable) {
    let verdict
    let note = ''
    try {
      sh('node', [driver, `orca/${c.name}.ts`, c.fn, c.argspec, `orca/${c.name}.rs`], {
        cwd: TS2RUST,
        env: { ...process.env, TRUSTC: trustc },
        timeout: 180000
      })
      verdict = 'TRUSTED'
    } catch (e) {
      const out = `${e.stdout || ''}${e.stderr || ''}`
      verdict = /VERDICT:\s*TRUSTED/u.test(out) ? 'TRUSTED' : 'NOT-TRUSTED'
      note = (
        out.split('\n').find((l) => /counterexample|divergence|ts=|rust=|REFUTED/iu.test(l)) || ''
      )
        .trim()
        .slice(0, 80)
    }
    rows.push({ fn: c.fn, argspec: c.argspec, expect: c.expect, verdict, note })
  }
  // A known-bug port coming back TRUSTED = soundness regression (FAIL). A faithful
  // port coming back NOT-TRUSTED = triage (port bug vs. a Trust verifier precision gap).
  const soundnessBreak = rows.some((r) => r.expect === 'NOT-TRUSTED' && r.verdict === 'TRUSTED')
  const faithfulMiss = rows.some((r) => r.expect === 'TRUSTED' && r.verdict === 'NOT-TRUSTED')
  return {
    status: soundnessBreak ? 'FAIL' : faithfulMiss ? 'REVIEW' : 'PASS',
    metrics: {
      trusted: rows.filter((r) => r.verdict === 'TRUSTED').length,
      total: rows.length,
      declined: corpus.length - runnable.length
    },
    rows
  }
}

// --- driver ----------------------------------------------------------------------
const GATES = { bootstrap, conformance, perf, safety, autoformalize }
const mark = (s) =>
  ({
    PASS: `${C.g}✓ PASS${C.x}`,
    FAIL: `${C.r}✗ FAIL${C.x}`,
    REVIEW: `${C.y}● REVIEW${C.x}`,
    SKIP: `${C.d}– SKIP${C.x}`
  })[s] ?? s

async function main() {
  const cmd = process.argv[2] || 'all'
  const names =
    cmd === 'all' ? ['bootstrap', 'conformance', 'perf', 'safety', 'autoformalize'] : [cmd]
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
      if (d.comment) {
        console.log(`      ${C.d}expected: ${d.comment}${C.x}`)
      }
    }
    for (const row of r.rows ?? []) {
      const hit = row.verdict === row.expect
      const col = row.verdict === 'TRUSTED' ? C.g : hit ? C.y : C.r
      console.log(
        `      ${col}${row.verdict}${C.x} ${row.fn}(${row.argspec}) ${C.d}expect ${row.expect}${row.note ? ` · ${row.note}` : ''}${C.x}`
      )
    }
  }
  writeFileSync(
    REPORT,
    JSON.stringify({ at: `${new Date().toISOString().slice(0, 19)}Z`, results }, null, 2)
  )
  console.log(`\n${C.d}report → ${REPORT}${C.x}`)
  const statuses = Object.values(results).map((r) => r.status)
  if (statuses.includes('FAIL')) {
    process.exit(1)
  }
  if (statuses.includes('REVIEW')) {
    process.exit(2)
  }
  const skips = statuses.filter((s) => s === 'SKIP').length
  if (skips > 0) {
    // A skipped gate proved nothing; only the full run must refuse to read green.
    if (cmd === 'all') {
      console.log(`${C.y}${C.b}${skips} gate(s) skipped — not a pass; exiting 2 (REVIEW)${C.x}`)
      process.exit(2)
    }
    console.log(
      `${C.y}${C.b}SKIPPED — this gate did not run and proves nothing (exit 0 only because a single gate was requested)${C.x}`
    )
  }
  process.exit(0)
}

main().catch((e) => {
  console.error(`${C.r}gauntlet crashed:${C.x} ${e.stack || e.message}`)
  process.exit(70)
})
