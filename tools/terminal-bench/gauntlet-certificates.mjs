// The moonshot E1 pair, enforced — the `certificates` gauntlet axis.
//
// E1 = "machine-checked safety certificates on the emitted code PLUS regression-
// gated behavioral parity corpora, in a shipping product." Both halves are enforced
// here for every decision-core crate that ships an ay certificate:
//   (a) discharge rust/crates/*/proofs/ay/verify.sh (success = exit 0)
//   (b) run that crate's Rust parity corpus (cargo test — matches_shared_parity_corpus)
// Auto-discovering: any new E1-unit crate (a proofs/ay/verify.sh) is picked up with
// no edit here. The TS side of each parity corpus runs in the vitest suite.
//
// Extracted from gauntlet.mjs to keep that file under its max-lines cap; the host
// passes in the shared primitives (repo root, sh, skip, rustupStable).

import { existsSync, readdirSync } from 'node:fs'
import { join } from 'node:path'

const findAy = (sh) => {
  const home = process.env.HOME ?? ''
  const cands = [
    join(home, '.cargo', 'bin', 'ay'),
    join(home, 'trust', 'build', 'host', 'stage2', 'bin', 'ay')
  ]
  for (const c of cands) {
    if (existsSync(c)) {
      return c
    }
  }
  try {
    return sh('command', ['-v', 'ay']).trim() || null
  } catch {
    return null
  }
}

export function certificatesGate({ repo, sh, skip, rustupStable }) {
  const cratesDir = join(repo, 'rust', 'crates')
  const crates = existsSync(cratesDir)
    ? readdirSync(cratesDir).filter((c) =>
        existsSync(join(cratesDir, c, 'proofs', 'ay', 'verify.sh'))
      )
    : []
  if (crates.length === 0) {
    return skip('no ay certificates found under rust/crates/*/proofs/ay')
  }
  const ay = findAy(sh)
  if (!ay) {
    return skip(
      `ay solver not found (~/.cargo/bin/ay, trust stage2, PATH) — ${crates.length} certificate(s) present but unproven; install ay then re-run`
    )
  }
  // (a) discharge every certificate. Success is the verify.sh EXIT CODE (0), not a
  // sentinel string — the certificates use different discharge banners (e.g.
  // orca-git/orca-net's Trust-parser bundles print "ALL BUNDLES DISCHARGED"). The
  // obligation tally counts per-line verdicts across both "ok …" and "  PASS …" forms.
  let obligations = 0
  const certFail = []
  for (const c of crates) {
    const vs = join(cratesDir, c, 'proofs', 'ay', 'verify.sh')
    try {
      const out = sh('bash', [vs])
      obligations += (out.match(/^\s*(?:ok|PASS)\b/gm) ?? []).length
    } catch (e) {
      certFail.push(`cert:${c} (verify.sh exit ${e.status ?? '?'})`)
    }
  }
  // (b) run the Rust parity corpora ONLY for crates that ship one (the TS→Rust
  // decision cores — orca-git/orca-net are proof-only Trust cores with no shared
  // corpus). Needs a stable toolchain (Homebrew rustc can shadow rustup); without
  // it the parity half is unverified, so the gate degrades to REVIEW rather than
  // reading green on the certificate half alone.
  const parityCrates = crates.filter((c) =>
    readdirSync(join(cratesDir, c)).some((f) => f.endsWith('parity-corpus.txt'))
  )
  const metricsBase = { crates: crates.length, obligations, parityCrates: parityCrates.length }
  const cargo = rustupStable('cargo')
  const rustc = rustupStable('rustc')
  if (parityCrates.length > 0 && !cargo) {
    return {
      status: 'REVIEW',
      metrics: { ...metricsBase, parity: 'not-run' },
      detail: `${obligations} ay obligations discharged across ${crates.length} crate(s), but the Rust parity corpora (${parityCrates.length} crate(s)) were NOT run (no stable rustup toolchain — run bootstrap). Certificate half proven; parity half unverified here.`
    }
  }
  let parityFail = null
  if (parityCrates.length > 0) {
    try {
      sh(cargo, ['test', '-q', ...parityCrates.flatMap((c) => ['-p', c])], {
        cwd: join(repo, 'rust'),
        env: { ...process.env, ...(rustc ? { RUSTC: rustc } : {}) }
      })
    } catch (e) {
      parityFail = `parity: cargo test exit ${e.status ?? '?'}`
    }
  }
  const fails = [...certFail, parityFail].filter(Boolean)
  if (fails.length > 0) {
    return {
      status: 'FAIL',
      metrics: { ...metricsBase, parity: parityFail ? 'FAIL' : 'pass' },
      detail: fails.join(' · ')
    }
  }
  return {
    status: 'PASS',
    metrics: { ...metricsBase, parity: 'pass' },
    detail: `E1 pair enforced: ${obligations} ay obligations discharged across ${crates.length} certificate crate(s) (${crates.join(', ')}) + Rust parity corpora green across ${parityCrates.length} decision-core crate(s). TS parity runs in the vitest suite.`
  }
}
