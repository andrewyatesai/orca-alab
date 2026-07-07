import { describe, expect, it } from 'vitest'
import { QUICK_OPEN_QUERY_MAX_BYTES, isQuickOpenQueryTooLarge } from './quick-open-search'

// The ranking tests moved to src/renderer/src/lib/git-wasm/quick-open.test.ts
// with the ranking's cutover to the Rust core; only the query-size guard
// (shared clipboard-text limit) remains TS.

describe('isQuickOpenQueryTooLarge', () => {
  it('accepts queries at the byte budget and rejects past it', () => {
    expect(isQuickOpenQueryTooLarge('a'.repeat(QUICK_OPEN_QUERY_MAX_BYTES))).toBe(false)
    expect(isQuickOpenQueryTooLarge('a'.repeat(QUICK_OPEN_QUERY_MAX_BYTES + 1))).toBe(true)
  })

  it('counts multibyte characters by UTF-8 bytes', () => {
    expect(isQuickOpenQueryTooLarge('\u{1F525}'.repeat(QUICK_OPEN_QUERY_MAX_BYTES / 4 + 1))).toBe(
      true
    )
  })
})
