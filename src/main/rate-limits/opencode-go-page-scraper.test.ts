import { describe, expect, it } from 'vitest'
import { parseSubscriptionFromPageText } from './opencode-go-page-scraper'

describe('parseSubscriptionFromPageText', () => {
  it('parses a well-formed usage payload', () => {
    const text =
      'noise rollingUsage:{usagePercent:12,resetInSec:3600} ' +
      'weeklyUsage:{usagePercent:34,resetInSec:7200} ' +
      'monthlyUsage:{usagePercent:56,resetInSec:9000} trailer'
    const parsed = parseSubscriptionFromPageText(text)
    expect(parsed).not.toBeNull()
    expect(parsed?.rollingUsagePercent).toBe(12)
    expect(parsed?.weeklyUsagePercent).toBe(34)
    expect(parsed?.monthlyUsagePercent).toBe(56)
    expect(parsed?.rollingResetInSec).toBe(3600)
  })

  it('fails closed fast on a hostile body of unbalanced key tokens (no quadratic freeze)', () => {
    // Why: a compromised opencode.ai page full of `rollingUsage:{` with no closing
    // brace previously made each occurrence scan to end-of-text — O(n^2).
    const hostile = 'rollingUsage:{'.repeat(120_000) // ~1.6MB, under the 2MB cap
    const start = Date.now()
    const parsed = parseSubscriptionFromPageText(hostile)
    const elapsedMs = Date.now() - start
    expect(parsed).toBeNull()
    expect(elapsedMs).toBeLessThan(1000)
  })

  it('rejects an oversized body before scanning', () => {
    const oversized = 'a'.repeat(2_000_001)
    const start = Date.now()
    expect(parseSubscriptionFromPageText(oversized)).toBeNull()
    expect(Date.now() - start).toBeLessThan(200)
  })
})
