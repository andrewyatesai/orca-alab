import { describe, expect, it } from 'vitest'
import { isBehindOnlyUpstream, shouldForcePushWithLeaseForUpstream } from './git-upstream-status'

// The upstreamOnlyCommitsArePatchEquivalent cases moved to
// src/relay/git-wasm.test.ts with the parser's cutover to the Rust core.

describe('shouldForcePushWithLeaseForUpstream', () => {
  it('requires a diverged upstream with patch-equivalent behind commits', () => {
    expect(
      shouldForcePushWithLeaseForUpstream({
        hasUpstream: true,
        ahead: 1,
        behind: 1,
        behindCommitsArePatchEquivalent: true
      })
    ).toBe(true)
    expect(
      shouldForcePushWithLeaseForUpstream({
        hasUpstream: true,
        ahead: 1,
        behind: 1,
        behindCommitsArePatchEquivalent: false
      })
    ).toBe(false)
  })
})

describe('isBehindOnlyUpstream', () => {
  it('is true only when the branch tracks upstream and is purely behind', () => {
    expect(
      isBehindOnlyUpstream({
        hasUpstream: true,
        ahead: 0,
        behind: 3
      })
    ).toBe(true)
    expect(
      isBehindOnlyUpstream({
        hasUpstream: true,
        ahead: 1,
        behind: 2
      })
    ).toBe(false)
    expect(
      isBehindOnlyUpstream({
        hasUpstream: true,
        ahead: 0,
        behind: 0
      })
    ).toBe(false)
    expect(
      isBehindOnlyUpstream({
        hasUpstream: false,
        ahead: 0,
        behind: 3
      })
    ).toBe(false)
    expect(isBehindOnlyUpstream(undefined)).toBe(false)
  })
})
