// The hand-coded-SMT guard must actually fail when a violation is planted.
//
// The bundles this guard replaces were deleted because they were a hand-maintained
// model that drifted from the code and hid five reachable integer-overflow bugs
// under "ALL PROOFS DISCHARGED". A guard that has never been seen to fail would be
// the same mistake in a new shape, so every case below plants a real violation on
// disk and watches the gate go red.

import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import { certificatesGate, findHandCodedSmt } from './gauntlet-certificates.mjs'

const skip = (detail) => ({ status: 'SKIP', detail })
// The parity half needs a toolchain we do not have in a unit test; returning no
// cargo makes the gate stop at REVIEW *after* the guard has passed, which is
// exactly the "guard green, parity unverified" branch we want to distinguish
// from a guard failure.
const noCargo = () => null

describe('certificates gate — the hand-coded-SMT guard', () => {
  let repo

  beforeEach(() => {
    repo = mkdtempSync(join(tmpdir(), 'cert-gate-'))
    mkdirSync(join(repo, 'rust', 'crates', 'orca-demo'), { recursive: true })
    // A crate with a parity corpus and no hand-coded SMT: the clean baseline.
    writeFileSync(join(repo, 'rust', 'crates', 'orca-demo', 'parity-corpus.txt'), '0 => 0\n')
  })

  afterEach(() => {
    rmSync(repo, { recursive: true, force: true })
  })

  const run = () => certificatesGate({ repo, sh: () => '', skip, rustupStable: noCargo })

  it('passes the guard when no hand-coded SMT is present', () => {
    const got = run()
    // Not PASS — the parity corpora cannot run here — but crucially not FAIL,
    // and the guard reports zero stray files.
    expect(got.status).toBe('REVIEW')
    expect(got.metrics.handCodedSmtFiles).toBe(0)
  })

  it('FAILS when a .smt2 file is planted under a crate', () => {
    const dir = join(repo, 'rust', 'crates', 'orca-demo', 'proofs', 'ay')
    mkdirSync(dir, { recursive: true })
    writeFileSync(join(dir, 'rh1_band_bound.smt2'), '(set-logic QF_LIA)\n(check-sat)\n')

    const got = run()
    expect(got.status).toBe('FAIL')
    expect(got.metrics.handCodedSmtFiles).toBe(1)
    expect(got.detail).toContain('rh1_band_bound.smt2')
    expect(got.detail).toContain('capability-gaps-from-the-smt-purge.md')
  })

  it('FAILS on an .alethe proof transcript too, and anywhere in the crate tree', () => {
    // Not under a `proofs/ay/` path — the guard is about the file kind, not a
    // directory name someone can rename around.
    const dir = join(repo, 'rust', 'crates', 'orca-demo', 'src', 'models')
    mkdirSync(dir, { recursive: true })
    writeFileSync(join(dir, 'ev1.smt2.alethe'), '(assume a0 true)\n')

    const got = run()
    expect(got.status).toBe('FAIL')
    expect(got.metrics.handCodedSmtFiles).toBe(1)
  })

  it('counts every violation across crates, not just the first', () => {
    for (const crate of ['orca-demo', 'orca-other']) {
      const dir = join(repo, 'rust', 'crates', crate, 'proofs', 'ay')
      mkdirSync(dir, { recursive: true })
      writeFileSync(join(dir, 'a.smt2'), '(check-sat)\n')
      writeFileSync(join(dir, 'b.smt2'), '(check-sat)\n')
    }
    const got = run()
    expect(got.status).toBe('FAIL')
    expect(got.metrics.handCodedSmtFiles).toBe(4)
  })

  it('ignores vendored and build trees, which are not ours to police', () => {
    for (const skipped of ['vendor', 'target', 'node_modules']) {
      const dir = join(repo, 'rust', 'crates', 'orca-demo', skipped)
      mkdirSync(dir, { recursive: true })
      writeFileSync(join(dir, 'third-party.smt2'), '(check-sat)\n')
    }
    expect(run().status).toBe('REVIEW')
    expect(findHandCodedSmt(join(repo, 'rust', 'crates'), repo)).toEqual([])
  })

  it('reports paths relative to the repo root so the detail is actionable', () => {
    const dir = join(repo, 'rust', 'crates', 'orca-demo', 'proofs', 'ay')
    mkdirSync(dir, { recursive: true })
    writeFileSync(join(dir, 'x.smt2'), '(check-sat)\n')

    expect(findHandCodedSmt(join(repo, 'rust', 'crates'), repo)).toEqual([
      join('rust', 'crates', 'orca-demo', 'proofs', 'ay', 'x.smt2')
    ])
  })
})
