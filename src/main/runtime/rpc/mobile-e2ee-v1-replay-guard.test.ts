import { describe, expect, it } from 'vitest'
import { MobileE2EEV1ReplayGuard } from './mobile-e2ee-v1-replay-guard'

function frameWithNonce(nonceByte: number, tail = 'payload'): Uint8Array {
  const nonce = new Uint8Array(24).fill(nonceByte)
  const body = new TextEncoder().encode(tail)
  const out = new Uint8Array(nonce.length + body.length)
  out.set(nonce, 0)
  out.set(body, nonce.length)
  return out
}

describe('MobileE2EEV1ReplayGuard', () => {
  it('accepts a first-seen nonce and rejects the same nonce on replay', () => {
    const guard = new MobileE2EEV1ReplayGuard()
    const frame = frameWithNonce(7)
    expect(guard.accept(frame)).toBe(true)
    // Byte-identical replay: same nonce prefix must be rejected.
    expect(guard.accept(frameWithNonce(7))).toBe(false)
    // A distinct nonce is still accepted.
    expect(guard.accept(frameWithNonce(8))).toBe(true)
  })

  it('handles base64 string frames identically to byte frames', () => {
    const guard = new MobileE2EEV1ReplayGuard()
    const b64 = Buffer.from(frameWithNonce(3)).toString('base64')
    expect(guard.accept(b64)).toBe(true)
    expect(guard.accept(b64)).toBe(false)
  })

  it('rejects frames too short to carry a nonce', () => {
    const guard = new MobileE2EEV1ReplayGuard()
    expect(guard.accept(new Uint8Array(23))).toBe(false)
  })

  it('evicts the oldest nonce once the window is full, but keeps recent ones', () => {
    const guard = new MobileE2EEV1ReplayGuard(2)
    expect(guard.accept(frameWithNonce(1))).toBe(true)
    expect(guard.accept(frameWithNonce(2))).toBe(true)
    // Overflow evicts nonce #1 (oldest); #2 is still tracked.
    expect(guard.accept(frameWithNonce(3))).toBe(true)
    expect(guard.accept(frameWithNonce(2))).toBe(false)
    // #1 was evicted, so a delayed replay past the window is re-accepted.
    expect(guard.accept(frameWithNonce(1))).toBe(true)
  })

  it('clear() forgets tracked nonces so the same frame is accepted again', () => {
    const guard = new MobileE2EEV1ReplayGuard()
    expect(guard.accept(frameWithNonce(9))).toBe(true)
    guard.clear()
    expect(guard.accept(frameWithNonce(9))).toBe(true)
  })
})
