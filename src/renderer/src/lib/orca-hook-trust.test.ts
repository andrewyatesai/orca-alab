import { afterEach, describe, expect, it } from 'vitest'
import { hashOrcaHookScript, isSharedOrcaCommandTrusted } from './orca-hook-trust'

const realCrypto = globalThis.crypto

afterEach(() => {
  Object.defineProperty(globalThis, 'crypto', { value: realCrypto, configurable: true })
})

function stubCrypto(value: unknown): void {
  Object.defineProperty(globalThis, 'crypto', { value, configurable: true })
}

describe('hashOrcaHookScript', () => {
  it('produces a stable hex digest via crypto.subtle', async () => {
    const hash = await hashOrcaHookScript('echo hi')
    expect(hash).toMatch(/^[0-9a-f]+$/)
    expect(await hashOrcaHookScript('  echo hi  ')).toBe(hash)
  })

  // Why: LAN web clients run on plain HTTP where crypto.subtle is undefined.
  // The hash must still compute (no "crypto.subtle is undefined" throw) and stay
  // deterministic so trust comparisons keep working.
  it('falls back to a deterministic hash when crypto.subtle is unavailable', async () => {
    stubCrypto(undefined)
    const hash = await hashOrcaHookScript('echo hi')
    expect(hash).toMatch(/^[0-9a-f]+$/)
    expect(await hashOrcaHookScript('echo hi')).toBe(hash)
    expect(await hashOrcaHookScript('echo bye')).not.toBe(hash)
  })
})

describe('isSharedOrcaCommandTrusted', () => {
  it('trusts only a matching setup content hash or an always-trusted repo', () => {
    expect(
      isSharedOrcaCommandTrusted({ setup: { contentHash: 'abc', approvedAt: 1 } }, 'abc')
    ).toBe(true)
    expect(
      isSharedOrcaCommandTrusted({ setup: { contentHash: 'abc', approvedAt: 1 } }, 'def')
    ).toBe(false)
    expect(isSharedOrcaCommandTrusted({ all: { approvedAt: 1 } }, 'anything')).toBe(true)
    expect(isSharedOrcaCommandTrusted(undefined, 'abc')).toBe(false)
  })

  // Why: fail closed — a snapshot Orca could not hash must never count as trusted.
  it('never trusts a missing content hash', () => {
    expect(isSharedOrcaCommandTrusted({ setup: { contentHash: '', approvedAt: 1 } }, null)).toBe(
      false
    )
    expect(isSharedOrcaCommandTrusted({ setup: { contentHash: '', approvedAt: 1 } }, '')).toBe(
      false
    )
    expect(isSharedOrcaCommandTrusted({ all: { approvedAt: 1 } }, null)).toBe(true)
  })
})
