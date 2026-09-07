// External corpus absence and incomplete harness evidence must never read as proof.

import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { rustTypeToArgspec } from './rust-type-to-argspec.mjs'

// Negative controls must match the suffix, not faithful names like `toobig`.
const CONTROL = /_(bug|naive)$/u

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
    // Borrowed-slice kernels may have a lifetime clause before their parameters.
    const sig = src.match(/pub\s+fn\s+(\w+)\s*(?:<[^>]*>)?\s*\(([^)]*)\)/u)
    if (!sig) {
      continue
    }
    // Tuple returns serialize to a JSON array, matching their TS twins.
    const params = sig[2].trim()
    const specs = []
    let ok = true
    // Trailing commas must not create an unsupported phantom parameter.
    for (const p of params
      ? params
          .split(',')
          .map((s) => s.trim())
          .filter(Boolean)
      : []) {
      const a = rustTypeToArgspec(p.split(':').slice(1).join(':'), src)
      if (!a) {
        ok = false
        break
      }
      specs.push(a)
    }
    if (ok) {
      out.push({
        name,
        fn: sig[1],
        argspec: specs.join(','),
        expect: CONTROL.test(name) ? 'NOT-TRUSTED' : 'TRUSTED'
      })
    } else {
      out.push({ name, fn: sig[1], declined: true })
    }
  }
  return out
}

function locateTrustc(trustRoot, sh) {
  const candidates = [
    process.env.TRUSTC,
    join(trustRoot, 'build', 'host', 'stage2', 'bin', 'trustc')
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

function readRatchet(file) {
  if (!existsSync(file)) {
    return { missing: true }
  }
  try {
    const ratchet = JSON.parse(readFileSync(file, 'utf8'))
    if (!ratchet || typeof ratchet !== 'object' || Array.isArray(ratchet)) {
      return { unreadable: 'expected a baseline object' }
    }
    return ratchet
  } catch (e) {
    return { unreadable: String(e.message).split('\n')[0] }
  }
}

function runKernel(sh, driver, ts2rust, trustc, c) {
  let out
  let code = 0
  try {
    out = sh(
      'node',
      [driver, join('orca', `${c.name}.ts`), c.fn, c.argspec, join('orca', `${c.name}.rs`)],
      {
        cwd: ts2rust,
        env: { ...process.env, TRUSTC: trustc },
        timeout: 180000
      }
    )
  } catch (e) {
    code = e.status ?? null
    out = `${e.stdout || ''}${e.stderr || ''}`
  }
  // Exit status and the unique harness verdict must agree; text alone proves nothing.
  const verdicts = [...out.matchAll(/^VERDICT:\s*(TRUSTED|NOT TRUSTED)\b[^\n]*$/gmu)]
  const declared = verdicts.length === 1 ? verdicts[0][1] : null
  const trusted = code === 0 && declared === 'TRUSTED'
  // INCOMPLETE and build errors are not negative-control refutations.
  const refuted =
    /^\[witness 1: STATIC PROOF\]\s+Verdict=REFUTED\b/mu.test(out) ||
    /^\[witness 2: DIFFERENTIAL\]\s+[1-9]\d* \/ [1-9]\d* divergences\b/mu.test(out)
  const rejected = code === 1 && declared === 'NOT TRUSTED' && refuted
  return {
    verdict: trusted ? 'TRUSTED' : rejected ? 'NOT-TRUSTED' : 'INCOMPLETE',
    exit: code,
    note: trusted
      ? ''
      : (
          out
            .split('\n')
            .find((l) => /counterexample|divergence|REFUTED|INCOMPLETE|ERROR/iu.test(l)) ||
          `harness exit ${code ?? 'unavailable'} without consistent proof/refutation evidence`
        )
          .trim()
          .slice(0, 160)
  }
}

export function autoformalizeGate({ here, sh, skip, trustRoot, trustRootLabel }) {
  const ts2rust = join(trustRoot, 'tools', 'ts2rust')
  const driver = join(ts2rust, 'autoformalize.mjs')
  if (!existsSync(driver)) {
    return skip(
      `Trust ts2rust harness not found (${trustRootLabel}/tools/ts2rust) — Goal A engine lives in the Trust repo; nothing ran, nothing proven`
    )
  }
  const ratchet = readRatchet(join(here, 'autoformalize-ratchet.json'))
  const num = (k, min) =>
    Number.isSafeInteger(ratchet[k]) && ratchet[k] >= min ? ratchet[k] : null
  const minTrusted = num('minTrusted', 0)
  const minControls = num('soundnessControls', 1)
  const where = `${trustRootLabel}/tools/ts2rust/orca`
  const corpus = discoverCorpus(join(ts2rust, 'orca'))
  const runnable = corpus.filter((c) => !c.declined)
  const controls = runnable.filter((c) => c.expect === 'NOT-TRUSTED').length
  // Empty discovery cannot satisfy a recorded baseline vacuously.
  if (!runnable.length) {
    const metrics = { corpus: 0, declined: corpus.length, minTrusted }
    return ratchet.missing
      ? {
          status: 'SKIP',
          metrics,
          detail: `0 autoformalizable .ts/.rs pairs discovered under ${where} — nothing ran, nothing proven`
        }
      : {
          status: 'FAIL',
          metrics,
          detail: `0 autoformalizable .ts/.rs pairs discovered under ${where} with a baseline on file — the ratcheted corpus is GONE; an empty corpus proves nothing`
        }
  }
  const trustc = locateTrustc(trustRoot, sh)
  if (!trustc) {
    return {
      status: 'SKIP',
      metrics: {
        corpus: runnable.length,
        declined: corpus.length - runnable.length,
        controls
      },
      detail: `trustc not built — ${runnable.length} orc functions ready to autoformalize; build the Trust stage2 toolchain or set TRUSTC=<path>, then re-run (nothing proven until then)`
    }
  }
  const rows = runnable.map((c) => ({
    name: c.name,
    fn: c.fn,
    argspec: c.argspec,
    expect: c.expect,
    ...runKernel(sh, driver, ts2rust, trustc, c)
  }))
  const trusted = rows.filter((r) => r.verdict === 'TRUSTED').length
  const broke = rows.filter((r) => r.expect === 'NOT-TRUSTED' && r.verdict === 'TRUSTED')
  const refutedControls = rows.filter(
    (r) => r.expect === 'NOT-TRUSTED' && r.verdict === 'NOT-TRUSTED'
  ).length
  const incompleteControls = rows.filter(
    (r) => r.expect === 'NOT-TRUSTED' && r.verdict === 'INCOMPLETE'
  )
  const faithfulMiss = rows.filter((r) => r.expect === 'TRUSTED' && r.verdict !== 'TRUSTED').length
  // A ratchet clause that is absent is a clause that never ran — REVIEW, not PASS.
  const unratcheted = [
    ratchet.missing ? 'no autoformalize-ratchet.json on disk' : null,
    ratchet.unreadable ? `autoformalize-ratchet.json unreadable (${ratchet.unreadable})` : null,
    !ratchet.missing && !ratchet.unreadable && minTrusted === null
      ? 'ratchet has no valid nonnegative integer minTrusted'
      : null,
    !ratchet.missing && !ratchet.unreadable && minControls === null
      ? 'ratchet has no valid positive integer soundnessControls'
      : null
  ].filter(Boolean)
  const fails = [
    broke.length
      ? `SOUNDNESS BREAK: known-bug port(s) came back TRUSTED — ${broke.map((r) => r.fn).join(', ')}`
      : null,
    minControls !== null && controls < minControls
      ? `soundness controls SHRANK: ${controls} < baseline ${minControls}${controls === 0 ? ' — ZERO controls left, so the soundness check above is vacuous' : ''}`
      : null,
    incompleteControls.length
      ? `soundness control(s) NOT REFUTED: ${incompleteControls.map((r) => r.name).join(', ')} — incomplete or failed harness runs prove nothing`
      : null,
    minTrusted !== null && trusted < minTrusted
      ? `TRUSTED count regressed: ${trusted} < baseline ${minTrusted} (a kernel stopped verifying, or the verifier changed — ratchet measured with ${ratchet.toolchain ?? 'an unrecorded toolchain'}, this run used ${trustcVersion(sh, trustc)})`
      : null
  ].filter(Boolean)
  return {
    status: fails.length ? 'FAIL' : unratcheted.length || faithfulMiss ? 'REVIEW' : 'PASS',
    metrics: {
      trusted,
      total: rows.length,
      declined: corpus.length - runnable.length,
      controls,
      refutedControls,
      minTrusted,
      minControls
    },
    detail:
      fails.join(' · ') ||
      (unratcheted.length
        ? `${trusted}/${rows.length} TRUSTED but NOT ratcheted: ${unratcheted.join('; ')} — write the baseline before calling this green`
        : `${trusted}/${rows.length} TRUSTED (baseline ${minTrusted}), ${refutedControls} soundness control(s) refuted${faithfulMiss ? `, ${faithfulMiss} faithful port(s) not proved — triage` : ''}${trusted > minTrusted ? ' — grew, bump baseline' : ''}`),
    rows
  }
}

function trustcVersion(sh, trustc) {
  try {
    return sh(trustc, ['--version']).trim()
  } catch {
    return trustc
  }
}
