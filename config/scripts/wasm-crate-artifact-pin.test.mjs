import { describe, expect, it } from 'vitest'
import {
  WASM_CRATE_PINS,
  buildCratePin,
  crateSourceSha256,
  decodeBase64Module,
  diffCratePin,
  verifyCratePin
} from './wasm-crate-artifact-pin.mjs'

describe('orca wasm crate artifact pins', () => {
  // The real guard: the committed crypto/git pins must match the committed
  // artifacts and the current crate source. A source edit without a rebuild, or a
  // half-regenerated base64/renderer pair, breaks this.
  for (const name of Object.keys(WASM_CRATE_PINS)) {
    it(`${name}: committed pin matches committed artifacts and crate source`, () => {
      expect(verifyCratePin(name)).toEqual([])
    })
  }

  it('crateSourceSha256 is deterministic', () => {
    const dir = WASM_CRATE_PINS.crypto.sourceDir
    expect(crateSourceSha256(dir)).toBe(crateSourceSha256(dir))
  })

  it('distinguishes the two crates by source hash', () => {
    expect(crateSourceSha256(WASM_CRATE_PINS.crypto.sourceDir)).not.toBe(
      crateSourceSha256(WASM_CRATE_PINS.git.sourceDir)
    )
  })

  it('decodeBase64Module recovers the embedded bytes', () => {
    const bytes = Buffer.from([0x00, 0x61, 0x73, 0x6d, 0x01, 0x02, 0x03])
    const module = `export const X =\n  '${bytes.toString('base64')}'\n`
    expect(Buffer.compare(decodeBase64Module(module), bytes)).toBe(0)
  })

  it('decodeBase64Module throws when no literal is present', () => {
    expect(() => decodeBase64Module('export const X = undefined\n')).toThrow(/no base64 literal/)
  })

  it('diffCratePin flags a tampered artifact SHA', () => {
    const pin = buildCratePin('crypto')
    const [firstArtifact] = Object.keys(pin.artifacts)
    pin.artifacts[firstArtifact] = { ...pin.artifacts[firstArtifact], sha256: 'deadbeef' }
    expect(diffCratePin('crypto', pin)).toContain(
      `${firstArtifact} does not match its size/SHA-256 pin`
    )
  })

  it('diffCratePin flags a stale source hash', () => {
    const pin = buildCratePin('git')
    pin.sourceSha256 = '0'.repeat(64)
    const mismatches = diffCratePin('git', pin)
    expect(
      mismatches.some((m) => m.includes('source changed since the artifacts were built'))
    ).toBe(true)
  })

  it('diffCratePin flags a base64/renderer wasm mismatch', () => {
    const pin = buildCratePin('crypto')
    pin.wasmSha256 = '1'.repeat(64)
    expect(diffCratePin('crypto', pin)).toContain(
      `${WASM_CRATE_PINS.crypto.rawWasm} does not match the pinned wasm SHA-256`
    )
  })

  it('diffCratePin rejects an unsupported pin shape', () => {
    expect(diffCratePin('git', { schema: 99 })).toEqual([
      'orca-git-wasm pin has an unsupported shape — run `pnpm build:relay-wasm`.'
    ])
  })
})
