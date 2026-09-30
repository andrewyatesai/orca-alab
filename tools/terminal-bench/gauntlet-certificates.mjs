// The moonshot E1 pair, enforced — the `certificates` gauntlet axis.
//
// E1 = "machine-checked safety certificates on the emitted code PLUS regression-
// gated behavioral parity corpora, in a shipping product." Both halves are enforced
// here for every decision-core crate that ships an ay certificate:
//   (a) discharge rust/crates/*/proofs/ay/verify.sh (success = exit 0)
//   (b) run that crate's Rust parity corpus (`targo --unverified test` —
//       matches_shared_parity_corpus)
// Auto-discovering: any new E1-unit crate (a proofs/ay/verify.sh) is picked up with
// no edit here. The TS side of each parity corpus runs in the vitest suite.
//
// Extracted from gauntlet.mjs to keep that file under its max-lines cap; the host
// passes in the shared primitives (repo root, sh, skip, resolveTargo).

import { existsSync, readdirSync } from 'node:fs'
import { join } from 'node:path'

// $AY → PATH (the atpkg-managed ay); never the Trust checkout's build/ tree, which is the
// compiler repo's exclusive build output.
const findAy = (sh) => {
  if (process.env.AY && existsSync(process.env.AY)) {
    return process.env.AY
  }
  try {
    return sh('bash', ['-lc', 'command -v ay']).trim() || null
  } catch {
    return null
  }
}

export function certificatesGate({ repo, sh, skip, resolveTargo }) {
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
      `ay solver not found ($AY, PATH) — ${crates.length} certificate(s) present but unproven; install ay (atpkg) then re-run`
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
      // Pin every bundle to the solver resolved above via its $AY rung.
      const out = sh('bash', [vs], { env: { ...process.env, AY: ay } })
      obligations += (out.match(/^\s*(?:ok|PASS)\b/gm) ?? []).length
    } catch (e) {
      certFail.push(`cert:${c} (verify.sh exit ${e.status ?? '?'})`)
    }
  }
  // (b) run the Rust parity corpora ONLY for crates that ship one (the TS→Rust
  // decision cores — orca-git/orca-net are proof-only Trust cores with no shared
  // corpus). Needs targo (the Trust toolchain); without it the parity half is
  // unverified, so the gate degrades to REVIEW rather than reading green on the
  // certificate half alone.
  // Scanned over EVERY crate, not over `crates`. `crates` is the ay-certificate
  // subset, so filtering it meant a decision core that ships a corpus but no
  // certificate was silently skipped — the corpus existed, the gate read green,
  // and nothing ran it. Discovery is by corpus, which is what the sentence above
  // has always claimed.
  const parityCrates = readdirSync(cratesDir).filter(
    (c) =>
      existsSync(join(cratesDir, c, 'Cargo.toml')) &&
      readdirSync(join(cratesDir, c)).some((f) => f.endsWith('parity-corpus.txt'))
  )
  const metricsBase = { crates: crates.length, obligations, parityCrates: parityCrates.length }
  const targo = parityCrates.length > 0 ? resolveTargo() : null
  if (parityCrates.length > 0 && !targo) {
    return {
      status: 'REVIEW',
      metrics: { ...metricsBase, parity: 'not-run' },
      detail: `${obligations} ay obligations discharged across ${crates.length} crate(s), but the Rust parity corpora (${parityCrates.length} crate(s)) were NOT run (targo, the Trust toolchain, not found). Certificate half proven; parity half unverified here.`
    }
  }
  let parityFail = null
  if (parityCrates.length > 0) {
    try {
      // Parity asks whether the ported logic matches its TS twin — a question
      // about the code, not the verifier — so it runs targo's unverified lane.
      sh(targo, ['--unverified', 'test', '-q', ...parityCrates.flatMap((c) => ['-p', c])], {
        cwd: join(repo, 'rust')
      })
    } catch (e) {
      parityFail = `parity: targo --unverified test exit ${e.status ?? '?'}`
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
