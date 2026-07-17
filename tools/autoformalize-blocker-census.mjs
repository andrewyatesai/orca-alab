#!/usr/bin/env node
// Autoformalize blocker census — classifies WHY each Goal-A faithful-miss fails
// trustc's W1 static-safety witness, so the "faithful-miss reservoir" breakdown is
// machine-reproducible instead of hand-analysed.
//
// A faithful-miss is a ported kernel that is W2-equivalent to its TS twin (0 fuzz
// divergences) but W1-INCOMPLETE (some safety obligation is not statically proven).
// This tool re-runs each kernel's W1 (the same serde-stripped lib wrap the driver
// uses) with `-Z trust-verify-output=json`, reads the per-obligation results, and
// buckets every UNPROVEN obligation into a blocker class. The classes distinguish
// the three FORMULATION-recoverable walls this factory already harvests from the
// genuine SOLVER residue:
//
//   absent-callee-alloc   to_string/collect/String::from — an `extern "Rust"`
//                         allocation trustc keeps fail-closed (it can OOM-unwind).
//                         RECOVERABLE by returning a borrowed &str / &'static str.
//   absent-callee-iter    chars/encode_utf16/split iterators — same absent-callee
//                         boundary; the ASCII-single-code subset is RECOVERABLE by a
//                         byte-scan (as_bytes + .get + saturating).
//   unsupported-mir-drop  drop-glue unwind of an owned String/Vec (`Drop::
//                         UnsupportedUnwind`) — the signature of a string BUILDER;
//                         avoidable only when the value is a borrowable substring.
//   unsupported-mir-*     slice bounds-check / arithmetic-overflow MIR the verifier
//                         does not model in-loop (BoundsCheck sidesteppable via .get).
//   division              a division/nonlinear VC (`_undef_` in an evaluable theory
//                         position) — genuine nonlinear-theory residue.
//   timeout               W1 did not finish under the per-kernel cap (a slow loop).
//   other                 anything else.
//
// A kernel whose ONLY blockers are absent-callee-* is formulation-recoverable; a
// kernel carrying an unsupported-mir/division/timeout blocker is (currently) solver
// residue. Verified root cause of the absent-callee wall:
// docs/rust-migration/extreme-performance-moonshot.md (trust_verify.rs:10473-10516).
//
// Usage:
//   node tools/autoformalize-blocker-census.mjs [--json] [--corpus <dir>] [--timeout-ms N]
// Env: TRUSTC=<path> (else the rustup stage2 default); TS2RUST_CORPUS=<dir>.
// SKIPs (exit 0, empty report) when trustc or the corpus is absent — the Goal-A
// engine + corpus live in the local ~/trust repo, so a fresh orc checkout has neither.

import { spawnSync } from 'node:child_process'
import { existsSync, readdirSync, readFileSync, writeFileSync, mkdirSync } from 'node:fs'
import { join } from 'node:path'
import { homedir, tmpdir } from 'node:os'

const args = process.argv.slice(2)
const jsonOut = args.includes('--json')
const corpusArg = argValue('--corpus')
const timeoutMs = Number(argValue('--timeout-ms') ?? 60000)

function argValue(flag) {
  const i = args.indexOf(flag)
  return i >= 0 && i + 1 < args.length ? args[i + 1] : null
}

function locateTrustc() {
  if (process.env.TRUSTC && existsSync(process.env.TRUSTC)) {
    return process.env.TRUSTC
  }
  const stage2 = join(homedir(), 'trust', 'build', 'host', 'stage2', 'bin', 'trustc')
  return existsSync(stage2) ? stage2 : null
}

const corpusDir =
  corpusArg ?? process.env.TS2RUST_CORPUS ?? join(homedir(), 'trust', 'tools', 'ts2rust', 'orca')
const trustc = locateTrustc()

function skip(reason) {
  const report = { status: 'SKIP', reason, kernels: 0 }
  if (jsonOut) {
    process.stdout.write(JSON.stringify(report))
  } else {
    console.log(`[blocker-census] SKIP — ${reason}`)
  }
  process.exit(0)
}

if (!trustc) {
  skip('trustc not built (set TRUSTC or build ~/trust stage2)')
}
if (!existsSync(corpusDir)) {
  skip(`corpus not found at ${corpusDir}`)
}

// --- W1 harness: mirror the driver's serde-strip + lib wrap so results match -----
function w1Lib(src) {
  const body = src
    .replace(/#\[derive\(([^)]*)\)\]/gu, (_m, inner) => {
      const keep = inner
        .split(',')
        .map((s) => s.trim())
        .filter((d) => d && !/(^|::)(serde|Serialize|Deserialize)\b/u.test(d))
      return keep.length ? `#[derive(${keep.join(', ')})]` : ''
    })
    .replace(/#\[serde\([^\]]*\)\]/gu, '')
  return `#![allow(non_snake_case, dead_code, unused)]\n${body}\n`
}

const OUT_DIR = join(tmpdir(), `af_blocker_${process.pid}`)
mkdirSync(OUT_DIR, { recursive: true })

function runW1(rsPath) {
  const libFile = join(OUT_DIR, 'lib.rs')
  writeFileSync(libFile, w1Lib(readFileSync(rsPath, 'utf8')))
  const r = spawnSync(
    trustc,
    [
      '--crate-type=lib',
      '--out-dir',
      OUT_DIR,
      '-Z',
      'trust-verify-output=json',
      '-A',
      'warnings',
      libFile
    ],
    { encoding: 'utf8', timeout: timeoutMs, maxBuffer: 1 << 26 }
  )
  if (r.error && r.error.code === 'ETIMEDOUT') {
    return { timedOut: true, unproven: [] }
  }
  const out = `${r.stdout ?? ''}${r.stderr ?? ''}`
  const unproven = []
  for (const line of out.split('\n')) {
    const i = line.indexOf('TRUST_JSON:')
    if (i < 0) {
      continue
    }
    let j
    try {
      j = JSON.parse(line.slice(i + 'TRUST_JSON:'.length))
    } catch {
      continue
    }
    if (j.type !== 'function_result') {
      continue
    }
    for (const res of j.results ?? []) {
      if (res.outcome !== 'proved') {
        unproven.push(res.description ?? '')
      }
    }
  }
  return { timedOut: false, unproven }
}

// --- classify one obligation description into a blocker class --------------------
function classify(desc) {
  // Drop-glue unwind of an owned String/Vec — a string builder's signature. Checked
  // before the MIR buckets because it is the dominant owned-alloc marker.
  if (/Drop::UnsupportedUnwind|drop glue/u.test(desc)) {
    return 'unsupported-mir-drop'
  }
  if (/absent callee|body not in the lowered bundle/u.test(desc)) {
    // Iterator adapters name their type (Chars/Bytes/Split/EncodeUtf16) or reach
    // through IntoIterator::into_iter — match case-insensitively on the type names.
    if (
      /[Cc]hars|[Bb]ytes|encode_?utf16|[Ss]plit|[Rr]split|IntoIterator|into_iter|Filter|Map<|char_indices/u.test(
        desc
      )
    ) {
      return 'absent-callee-iter'
    }
    if (
      /to_string|to_owned|::collect\b|String::from|ToString|push_str|::push\b|from_utf16/u.test(
        desc
      )
    ) {
      return 'absent-callee-alloc'
    }
    return 'absent-callee-other'
  }
  if (/unsupported MIR.*BoundsCheck|slice bounds check/u.test(desc)) {
    return 'unsupported-mir-bounds'
  }
  if (/unsupported MIR.*ArithmeticSafety|arithmetic overflow/u.test(desc)) {
    return 'unsupported-mir-arith'
  }
  if (/_undef_|division|evaluable theory position|nonlinear/u.test(desc)) {
    return 'division'
  }
  return 'other'
}

// --- discover runnable kernels (same shape as the gauntlet: .rs with a .ts twin) -
const kernels = readdirSync(corpusDir)
  .filter((f) => f.endsWith('.rs'))
  .map((f) => f.slice(0, -3))
  .filter((name) => existsSync(join(corpusDir, `${name}.ts`)))
  .sort()

const FORMULATION = new Set(['absent-callee-alloc', 'absent-callee-iter'])
const catCounts = {}
const perKernel = []
let verified = 0

for (const name of kernels) {
  const { timedOut, unproven } = runW1(join(corpusDir, `${name}.rs`))
  const cats = new Set(timedOut ? ['timeout'] : unproven.map(classify))
  if (cats.size === 0) {
    verified += 1
    continue
  }
  for (const c of cats) {
    catCounts[c] = (catCounts[c] ?? 0) + 1
  }
  // Formulation-recoverable = every blocker is an absent-callee-alloc/iter.
  const recoverable = [...cats].every((c) => FORMULATION.has(c))
  perKernel.push({ name, blockers: [...cats].sort(), recoverable })
}

const w1Incomplete = perKernel.length
const recoverable = perKernel.filter((k) => k.recoverable).length
const report = {
  status: 'OK',
  corpus: corpusDir,
  kernels: kernels.length,
  w1Verified: verified,
  w1Incomplete,
  // kernels whose ONLY W1 blockers are absent-callee allocation/iterator (a
  // formulation fix — &str/byte-scan — not a solver capability)
  formulationRecoverable: recoverable,
  // the genuine solver residue (carries an unsupported-mir / division / timeout blocker)
  solverResidue: w1Incomplete - recoverable,
  byCategory: Object.fromEntries(Object.entries(catCounts).sort((a, b) => b[1] - a[1]))
}

if (jsonOut) {
  process.stdout.write(JSON.stringify({ ...report, perKernel }))
} else {
  console.log(`[blocker-census] ${corpusDir}`)
  console.log(
    `  kernels ${report.kernels} · W1-verified ${verified} · W1-incomplete ${w1Incomplete}`
  )
  console.log(
    `  formulation-recoverable ${recoverable} (absent-callee alloc/iter only) · solver-residue ${report.solverResidue}`
  )
  console.log('  by blocker class (kernels carrying it):')
  for (const [c, n] of Object.entries(report.byCategory)) {
    console.log(`    ${c.padEnd(24)} ${n}`)
  }
}
