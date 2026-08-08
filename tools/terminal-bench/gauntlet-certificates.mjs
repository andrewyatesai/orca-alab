// The moonshot E1 pair, enforced — the `certificates` gauntlet axis.
//
// E1 = "machine-checked safety certificates on the emitted code PLUS regression-
// gated behavioral parity corpora, in a shipping product."
//
// WHAT CHANGED (the SMT purge). The certificate half used to mean "discharge each
// decision-core crate's hand-written rust/crates/*/proofs/ay/*.smt2 bundle". Those
// bundles were deleted: they modelled the decision logic in QF_LIA over UNBOUNDED
// `Int`, so machine-integer overflow was outside every one of them, and five
// reachable i64/u64 overflow bugs sat under "ALL PROOFS DISCHARGED" until the
// compiler's own verifier refuted them. A model that restates the code by hand is
// not a check on the code; it is a second thing to keep in sync, and it drifted.
//
// The certificate half is now (a) the COMPILER's verifier, which runs on every
// build of rust/ via -Ztrust-verify=on and sees the real machine types, and (b) the
// in-source property tests that replaced each retired theorem. This axis therefore
// enforces the invariant that keeps (a) honest: no hand-encoded SMT may come back.
//
// Extracted from gauntlet.mjs to keep that file under its max-lines cap; the host
// passes in the shared primitives (repo root, sh, skip, rustupStable).

import { existsSync, readdirSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'

// Anything a human hand-writes for a solver. `.alethe` is an ay proof transcript,
// `verify.sh` inside a proofs dir is the discharge driver for such a bundle.
const SMT_FILE = /\.(smt2|alethe)$/

/** Every hand-coded solver input under `dir`, repo-relative. */
export function findHandCodedSmt(dir, repo, out = []) {
  let entries
  try {
    entries = readdirSync(dir, { withFileTypes: true })
  } catch {
    return out
  }
  for (const e of entries) {
    const p = join(dir, e.name)
    if (e.isDirectory()) {
      if (e.name === 'target' || e.name === 'node_modules' || e.name === 'vendor') {
        continue
      }
      findHandCodedSmt(p, repo, out)
      continue
    }
    if (SMT_FILE.test(e.name)) {
      out.push(relative(repo, p))
    }
  }
  return out
}

export function certificatesGate({ repo, sh, skip, rustupStable }) {
  const cratesDir = join(repo, 'rust', 'crates')
  if (!existsSync(cratesDir)) {
    return skip('rust/crates not found')
  }

  // (a) THE GUARD. Hand-encoded SMT under a first-party crate is a regression:
  // it re-introduces a hand-maintained model of code the compiler already
  // verifies, and such a model silently stops matching the code it claims to
  // describe. Route the property through the trust-* stack instead — an in-source
  // property test, a contract, or a capability ask in
  // docs/trust/capability-gaps-from-the-smt-purge.md if the stack cannot express
  // it yet.
  const strays = findHandCodedSmt(cratesDir, repo)
  if (strays.length > 0) {
    return {
      status: 'FAIL',
      metrics: { handCodedSmtFiles: strays.length },
      detail:
        `hand-coded SMT is banned under rust/crates — found ${strays.length}: ` +
        `${strays.slice(0, 6).join(', ')}${strays.length > 6 ? ', …' : ''}. ` +
        'Express the property through the trust-* stack (in-source test/contract), ' +
        'or record why it cannot be in docs/trust/capability-gaps-from-the-smt-purge.md.'
    }
  }

  // (b) the parity corpora — the behavioral half of E1, unchanged. These are the
  // shared TS↔Rust oracles; the TS side runs in the vitest suite.
  const parityCrates = readdirSync(cratesDir).filter((c) => {
    const p = join(cratesDir, c)
    try {
      return (
        statSync(p).isDirectory() && readdirSync(p).some((f) => f.endsWith('parity-corpus.txt'))
      )
    } catch {
      return false
    }
  })
  if (parityCrates.length === 0) {
    return skip('no parity corpora found under rust/crates/*/parity-corpus.txt')
  }

  // Needs a stable toolchain (Homebrew rustc can shadow rustup); without it the
  // parity half is unverified, so the gate degrades to REVIEW rather than reading
  // green on the guard alone.
  const cargo = rustupStable('cargo')
  const rustc = rustupStable('rustc')
  const metricsBase = { handCodedSmtFiles: 0, parityCrates: parityCrates.length }
  if (!cargo) {
    return {
      status: 'REVIEW',
      metrics: { ...metricsBase, parity: 'not-run' },
      detail: `no hand-coded SMT under rust/crates (guard green), but the Rust parity corpora (${parityCrates.length} crate(s)) were NOT run (no stable rustup toolchain — run bootstrap).`
    }
  }

  try {
    sh(cargo, ['test', '-q', ...parityCrates.flatMap((c) => ['-p', c])], {
      cwd: join(repo, 'rust'),
      env: {
        ...process.env,
        ...(rustc ? { RUSTC: rustc } : {}),
        // The repo default toolchain is `trust`, which ships no `rustc` binary
        // under that name, so a bare stable cargo still re-resolves to it and dies
        // on `rustc -vV`. Pin the toolchain, and clear the `-Z` rustflags that
        // rust/.cargo/config.toml sets for trustc — stable rejects them outright.
        RUSTUP_TOOLCHAIN: 'stable',
        RUSTFLAGS: '',
        RUSTDOCFLAGS: ''
      }
    })
  } catch (e) {
    return {
      status: 'FAIL',
      metrics: { ...metricsBase, parity: 'FAIL' },
      detail: `parity: cargo test exit ${e.status ?? '?'}`
    }
  }

  return {
    status: 'PASS',
    metrics: { ...metricsBase, parity: 'pass' },
    detail: `E1 pair enforced: no hand-coded SMT under rust/crates (the compiler's own verifier carries the certificate half via -Ztrust-verify=on) + Rust parity corpora green across ${parityCrates.length} decision-core crate(s) (${parityCrates.join(', ')}). TS parity runs in the vitest suite.`
  }
}
