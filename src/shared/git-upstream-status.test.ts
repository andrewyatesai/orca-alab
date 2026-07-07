import { describe, expect, it } from 'vitest'
import { shouldForcePushWithLeaseForUpstream } from './git-upstream-status'

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
