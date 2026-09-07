#!/usr/bin/env node
// aterm superiority gauntlet — the agent-runnable gate that REPLACES CI (no GitHub Actions).
//
// Proves the hand-written Rust aterm engine (shipped as `orca-terminal` via the
// native/orca-node napi addon) is a FULL, SUPERIOR replacement for @xterm/headless
// across the three axes from tools/aterm-vs-xterm/GOAL-B-HANDOFF.md:
//   • bootstrap   — installs the three prerequisites (napi addon, @xterm/headless
//                   oracle, perf corpus) and then VERIFIES them by loading/measuring
//                   (gauntlet-prereqs.mjs). Present-but-unusable = FAIL; unavailable
//                   toolchain = REVIEW. `bootstrap --verify` checks without installing.
//   • conformance — visible-grid parity per ANSI case. aterm matching xterm = parity;
//                   a divergence is REVIEW, not auto-fail, because "more correct than
//                   xterm per the VT/ECMA-48 spec" is a WIN to be triaged, not a bug.
//   • perf        — MB/s throughput vs xterm, best-of-N medians in one thermal state.
//   • safety      — Trust-proved obligations (skipped, not failed, when the toolchain is absent).
//   • autoformalize — Goal A: reuse the Trust ts2rust two-witness gate ($TRUST_REPO/tools/ts2rust)
//                   to prove the orc corpus's Rust ports refine their TS (skipped if trustc absent).
//   • census      — generated inventory ratchet (tools/repo-census.mjs): the delivery-shim
//                   and god-object regret class may only shrink; growth is REVIEW to triage
//                   (update census-ratchet.json knowingly — `_`-prefixed keys there record
//                   why a re-baseline was accepted), and every run snapshots the full
//                   inventory into the report so drift history accrues.
//   • provenance  — every TS→Rust ported module pinned to its source hashes
//                   (tools/port-provenance.mjs vs port-provenance.json): upstream TS drift
//                   is REVIEW with a structured re-port task, not a reactive parity surprise.
//   • certificates — the moonshot E1 pair, ENFORCED: discharge every decision-core crate's
//                   ay certificate (rust/crates/*/proofs/ay/verify.sh) AND run its Rust parity
//                   corpus. Auto-discovering; skipped (proves nothing) when ay is absent.
//   • corpus      — the parity-corpus ratchet (moonshot F2): the machine-checked behavioral
//                   parity case count (tools/parity-corpus-metrics.mjs vs parity-corpus-baseline.json)
//                   may only GROW; a drop FAILs (a corpus was deleted/shrunk).
//
// An agent runs:  node tools/terminal-bench/gauntlet.mjs <bootstrap|conformance|perf|safety|autoformalize|census|provenance|certificates|corpus|all> [--verify]
// Exit 0 = every selected gate proved green · 1 = a real FAIL · 2 = REVIEW (a
// divergence to triage, or a run that skipped some axis) · 3 = NOTHING PROVEN
// (every selected gate skipped). A SKIP never FAILs — the toolchain may simply be
// absent — but it never reads as green either, single-gate probes included; see
// gauntlet-exit-code.mjs. A machine-readable report (with `exit`/`summary`) is
// written to tools/terminal-bench/.gauntlet-report.json.

import { execFileSync } from 'node:child_process'
import { readFileSync, writeFileSync, existsSync, mkdirSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, join, resolve } from 'node:path'
import { tmpdir } from 'node:os'
import { loadCorpus, loadJsonlCorpus } from '../aterm-vs-xterm/corpus-bytes.mjs'
import { certificatesGate } from './gauntlet-certificates.mjs'
import { autoformalizeGate } from './gauntlet-autoformalize.mjs'
import { censusGate } from './gauntlet-census.mjs'
import { corpusGate } from './gauntlet-corpus.mjs'
import { EXIT, gauntletExit } from './gauntlet-exit-code.mjs'
import { PERF_CORPUS_MB, prereqChecks } from './gauntlet-prereqs.mjs'

const here = import.meta.dirname
const repo = resolve(here, '..', '..')
const require = createRequire(import.meta.url)

const ADDON = join(repo, 'native', 'orca-node', 'orca_node.node')
const XTERM = join(here, 'node_modules', '@xterm', 'headless', 'lib-headless', 'xterm-headless.js')
const BENCH_MANIFEST = join(here, 'package.json')
const CONF_CORPUS = join(repo, 'tools', 'aterm-vs-xterm', 'corpus.json')
const CONF_JSONL = join(repo, 'tools', 'conformance', 'cases.jsonl')
const BENCH_DIR = join(tmpdir(), 'orca-bench')
const PERF_CORPUS = join(BENCH_DIR, 'corpus.bin')
const REPORT = join(here, '.gauntlet-report.json')
const PERF_FLOOR = 1 // aterm must be at least as fast as xterm; the real ratio is reported.

const rstrip = (s) => s.replace(/\s+$/u, '')
const sh = (cmd, args, opts = {}) =>
  execFileSync(cmd, args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], ...opts })
const skip = (reason) => ({ status: 'SKIP', detail: reason })
const C = { g: '\x1b[32m', r: '\x1b[31m', y: '\x1b[33m', d: '\x1b[2m', b: '\x1b[1m', x: '\x1b[0m' }

// --- bootstrap: make every prerequisite present, idempotently — then PROVE it -----
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

// `--verify`: report what is bootstrapped without installing anything — an agent
// probing a cold tree should not silently kick off a multi-minute cargo build.
const verifyOnly = process.argv.includes('--verify')

function bootstrap() {
  mkdirSync(BENCH_DIR, { recursive: true })
  const did = []
  const blocked = []
  // An install that fails is a reported status, not a crash that aborts the whole run.
  const install = (what, how, run) => {
    if (verifyOnly) {
      blocked.push(`${what} not installed (--verify)`)
      return
    }
    console.log(`${C.d}  ${how}…${C.x}`)
    try {
      run()
      did.push(what)
    } catch (e) {
      blocked.push(`${what} BLOCKED — ${String(e.message).split('\n')[0]}`)
    }
  }
  if (!existsSync(ADDON)) {
    // The build script owns the cdylib→orca_node.node rename, submodule init and
    // toolchain pinning — a raw `cargo build` here would leave ADDON missing.
    install('napi addon', 'building napi addon (config/scripts/build-terminal-addon.mjs)', () =>
      sh('node', [join(repo, 'config', 'scripts', 'build-terminal-addon.mjs'), '--if-missing'], {
        stdio: 'inherit'
      })
    )
  }
  if (!existsSync(XTERM)) {
    install(
      '@xterm/headless baseline',
      'installing @xterm/headless baseline (pnpm install in tools/terminal-bench)',
      // --ignore-workspace is load-bearing: tools/terminal-bench is NOT a member of
      // the root pnpm-workspace.yaml, so a bare `pnpm -C <here> install` walks up,
      // installs the ROOT workspace instead (its postinstall and husky prepare both
      // fire), reports "Done", and leaves @xterm/headless uninstalled. That is why
      // this leg reported MISSING no matter how often bootstrap ran, and why the
      // conformance axis could never execute.
      () => sh('pnpm', ['-C', here, 'install', '--ignore-workspace'], { stdio: 'inherit' })
    )
  }
  if (!existsSync(PERF_CORPUS)) {
    // The prebuilt napi addon still runs if this is blocked; only the from-source
    // rebuild needs cargo (rust/vendor carries the full lockfile closure, so it
    // should build offline).
    install(
      'perf corpus',
      `generating ${PERF_CORPUS_MB} MB perf corpus (orca-terminal bench example)`,
      () => {
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
            String(PERF_CORPUS_MB)
          ],
          {
            cwd: repo,
            stdio: 'inherit',
            env: { ...process.env, CARGO_NET_OFFLINE: 'false', ...(rustc ? { RUSTC: rustc } : {}) }
          }
        )
      }
    )
  }
  // existsSync proved a path, not a prerequisite — load/measure every artifact.
  const checks = prereqChecks({
    addon: ADDON,
    xtermEntry: XTERM,
    benchManifest: BENCH_MANIFEST,
    corpus: PERF_CORPUS
  })
  const broken = checks.filter((c) => !c.ok && c.present)
  const absent = checks.filter((c) => !c.ok && !c.present)
  const summary = broken.length
    ? `${broken.length}/${checks.length} prerequisites present but UNUSABLE`
    : absent.length
      ? `${absent.length}/${checks.length} prerequisites unavailable on this machine`
      : `all ${checks.length} prerequisites verified (not just present)`
  return {
    status: broken.length ? 'FAIL' : absent.length || blocked.length ? 'REVIEW' : 'PASS',
    metrics: Object.fromEntries(
      checks.map((c) => [c.name, c.ok ? c.info : c.present ? 'BROKEN' : 'MISSING'])
    ),
    detail: [
      summary,
      did.length ? `installed: ${did.join(', ')}` : null,
      ...[...broken, ...absent].map((c) => `${c.name}: ${c.reason}`),
      ...blocked
    ]
      .filter(Boolean)
      .join(' · '),
    checks
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
  // Base corpus (80x24, base64) + the spec-cited differential corpus (per-case
  // dims, hex, xterm goldens) folded in — both feed the same aterm-vs-xterm text
  // differential. The jsonl cases carry `cols`/`rows`; the base cases default to
  // 80x24 (many base cases depend on the col-80 wrap boundary).
  const cases = [
    ...loadCorpus(CONF_CORPUS),
    ...(existsSync(CONF_JSONL) ? loadJsonlCorpus(CONF_JSONL) : [])
  ]
  let parity = 0
  const diverge = []
  const expectedDivergences = []
  for (const {
    name,
    bytes: buf,
    comment,
    cols,
    rows: caseRows,
    expected_divergence: expected
  } of cases) {
    const nCols = cols ?? 80
    const nRows = caseRows ?? 24
    const rt = new HeadlessTerminal(nCols, nRows, 1000)
    rt.write(buf)
    const a = rt.snapshot().map(rstrip)
    const xt = new Terminal({ rows: nRows, cols: nCols, allowProposedApi: true })
    xt.write(buf)
    await new Promise((r) => xt.write('', r)) // xterm flush
    const x = []
    // xterm's getLine is buffer-absolute (scrollback included); aterm's snapshot
    // is the visible viewport — offset by viewportY or any case that scrolls
    // misreports a divergence.
    const viewportY = xt.buffer.active.viewportY
    for (let r = 0; r < nRows; r++) {
      x.push(rstrip(xt.buffer.active.getLine(viewportY + r)?.translateToString(true) ?? ''))
    }
    if (a.join('\n') === x.join('\n')) {
      parity++
      if (expected) {
        // An accepted divergence that stopped diverging needs re-triage: either
        // the baseline fixed itself or the corpus case rotted.
        diverge.push({
          name,
          comment: `expected divergence NO LONGER reproduces — re-triage: ${comment ?? ''}`,
          rows: []
        })
      }
    } else {
      const rows = []
      for (let r = 0; r < nRows && rows.length < 4; r++) {
        if (a[r] !== x[r]) {
          rows.push({ row: r, aterm: (a[r] ?? '').slice(0, 60), xterm: (x[r] ?? '').slice(0, 60) })
        }
      }
      // Triaged, spec-cited divergences (expected_divergence in the corpus, verdicts
      // in tools/aterm-vs-xterm/TRIAGE.md) are accepted rather than REVIEW — the
      // classic conformance-suite expected-fail list. Anything untriaged is REVIEW.
      if (expected) {
        expectedDivergences.push(comment ? { name, comment, rows } : { name, rows })
      } else {
        diverge.push(comment ? { name, comment, rows } : { name, rows })
      }
    }
  }
  return {
    status: diverge.length === 0 ? 'PASS' : 'REVIEW',
    metrics: {
      parity,
      total: cases.length,
      divergences: diverge.length,
      accepted_divergences: expectedDivergences.length
    },
    diverge,
    expectedDivergences
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
// The Trust checkout lives outside this repo and its location is per-machine, so
// it is resolved from $TRUST_REPO (default: `trust` under $HOME) and never hard-coded.
const TRUST_ROOT = process.env.TRUST_REPO || join(process.env.HOME || '', 'trust')
const TRUST_ROOT_LABEL = '$TRUST_REPO'

// Same ladder as proofs/ay/resolve-solver.sh: $AY → PATH → the canonical cargo
// symlink → in-tree trust bootstrap outputs.
function locateAy() {
  const home = process.env.HOME || ''
  if (process.env.AY && existsSync(process.env.AY)) {
    return process.env.AY
  }
  try {
    const onPath = sh('bash', ['-lc', 'command -v ay']).trim()
    if (onPath) {
      return onPath
    }
  } catch {
    // not on PATH — fall through to the known build locations
  }
  const candidates = [
    join(home, '.cargo', 'bin', 'ay'),
    join(TRUST_ROOT, 'build', 'host', 'stage2', 'bin', 'ay'),
    join(
      TRUST_ROOT,
      'build',
      'aarch64-apple-darwin',
      'stage3-tools-bin',
      'aarch64-apple-darwin',
      'ay'
    ),
    join(
      TRUST_ROOT,
      'build',
      'aarch64-apple-darwin',
      'stage2-tools-bin',
      'aarch64-apple-darwin',
      'ay'
    )
  ]
  return candidates.find((c) => existsSync(c)) ?? null
}

function safety() {
  const ay = locateAy()
  const verify = join(repo, 'rust', 'crates', 'orca-git', 'proofs', 'ay', 'verify.sh')
  if (!ay) {
    return skip('Trust solver `ay` not found (~/.cargo/bin/ay) — safety axis unavailable here')
  }
  if (!existsSync(verify)) {
    return skip('orca-git proof bundle (proofs/ay/verify.sh) not found')
  }
  try {
    // Pin the bundles to the resolved binary via the ladder's $AY step.
    const out = sh('bash', [verify], { cwd: dirname(verify), env: { ...process.env, AY: ay } })
    const discharged = (out.match(/^\s*PASS\s/gm) || []).length
    const clean = /DISCHARGED/.test(out) && !/\b(FAIL|UNKNOWN|error)\b/i.test(out)
    return {
      status: clean ? 'PASS' : 'REVIEW',
      metrics: { obligations_discharged: discharged },
      detail: 'orca-git SMT obligations (tcargo panic/UB proofs need the full Trust toolchain)'
    }
  } catch (e) {
    return { status: 'FAIL', detail: String(e.message).split('\n')[0] }
  }
}

// --- autoformalize: fail-closed Trust corpus and evidence ratchets ---------------
const autoformalize = () =>
  autoformalizeGate({ here, sh, skip, trustRoot: TRUST_ROOT, trustRootLabel: TRUST_ROOT_LABEL })

// --- provenance: every TS→Rust port pinned to its source hashes -------------------
// Drift in a ported module's TS reference (or its Rust twin) must fail loudly with
// a structured re-port task (moonshot F1) — parity only catches it reactively.
function provenance() {
  let out
  let code = 0
  try {
    out = sh('node', [join(repo, 'tools', 'port-provenance.mjs'), '--json'])
  } catch (e) {
    code = e.status ?? 1
    out = `${e.stdout ?? ''}`
  }
  let report
  try {
    report = JSON.parse(out)
  } catch {
    return { status: 'FAIL', detail: `port-provenance checker emitted no JSON (exit ${code})` }
  }
  const metrics = { ...report.stats, drifts: report.drifts.length }
  if (code === 0) {
    return {
      status: 'PASS',
      metrics,
      detail: 'every ported module matches its pinned TS/Rust source hashes'
    }
  }
  if (code === 2) {
    const tasks = report.drifts.map((d) =>
      [
        `${d.kind}: ${d.module}`,
        d.file,
        d.rustTwins?.length ? `re-port ${d.rustTwins.join(', ')}` : null,
        d.vectors ? `re-verify ${d.vectors}` : null
      ]
        .filter(Boolean)
        .join(' — ')
    )
    const shown = tasks.slice(0, 6).join(' · ')
    return {
      status: 'REVIEW',
      metrics,
      detail: tasks.length > 6 ? `${shown} · +${tasks.length - 6} more (see report)` : shown,
      drifts: report.drifts
    }
  }
  return { status: 'FAIL', detail: `port-provenance checker broke (exit ${code})` }
}

// --- certificates: the moonshot E1 pair, enforced (extracted module) -------------
const certificates = () => certificatesGate({ repo, sh, skip, rustupStable })

// --- corpus: the parity-corpus ratchet (moonshot F2, extracted module) -----------
const census = () => censusGate({ repo, here, benchDir: BENCH_DIR, sh })
const corpus = () => corpusGate({ repo, sh })

// --- driver ----------------------------------------------------------------------
const GATES = {
  bootstrap,
  conformance,
  perf,
  safety,
  autoformalize,
  census,
  provenance,
  certificates,
  corpus
}
const mark = (s) =>
  ({
    PASS: `${C.g}✓ PASS${C.x}`,
    FAIL: `${C.r}✗ FAIL${C.x}`,
    REVIEW: `${C.y}● REVIEW${C.x}`,
    SKIP: `${C.d}– SKIP${C.x}`
  })[s] ?? s

async function main() {
  const arg = process.argv[2]
  const cmd = !arg || arg.startsWith('-') ? 'all' : arg
  const names =
    cmd === 'all'
      ? [
          'bootstrap',
          'census',
          'provenance',
          'certificates',
          'corpus',
          'conformance',
          'perf',
          'safety',
          'autoformalize'
        ]
      : [cmd]
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
        console.log(`      ${C.d}${d.comment}${C.x}`)
      }
    }
    for (const d of r.expectedDivergences ?? []) {
      console.log(`      ${C.d}accepted divergence: ${d.name} — ${d.comment ?? ''}${C.x}`)
    }
    for (const row of r.rows ?? []) {
      const hit = row.verdict === row.expect
      const col = row.verdict === 'TRUSTED' ? C.g : hit ? C.y : C.r
      console.log(
        `      ${col}${row.verdict}${C.x} ${row.fn}(${row.argspec}) ${C.d}expect ${row.expect}${row.note ? ` · ${row.note}` : ''}${C.x}`
      )
    }
  }
  const { code, line } = gauntletExit(Object.values(results).map((r) => r.status))
  writeFileSync(
    REPORT,
    JSON.stringify(
      { at: `${new Date().toISOString().slice(0, 19)}Z`, exit: code, summary: line, results },
      null,
      2
    )
  )
  console.log(`\n${C.d}report → ${REPORT}${C.x}`)
  const tone = code === EXIT.pass ? C.g : code === EXIT.fail ? C.r : C.y
  console.log(`${tone}${C.b}${line}${C.x}`)
  process.exit(code)
}

main().catch((e) => {
  console.error(`${C.r}gauntlet crashed:${C.x} ${e.stack || e.message}`)
  process.exit(70)
})
