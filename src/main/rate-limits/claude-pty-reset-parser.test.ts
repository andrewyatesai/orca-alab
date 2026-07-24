// Why: pin that the weekday and time-only reset branches honor the IANA zone in
// the reset line, matching the month-day branch. Fix the local zone to New York
// so a Los Angeles reset line resolves to the LA wall clock, not the local one.
process.env.TZ = 'America/New_York'

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { extractClaudePtyResetMetadata } from './claude-pty-reset-parser'

function parseReset(resetLine: string): {
  resetsAt: number | null
  resetDescription: string | null
} {
  return extractClaudePtyResetMetadata(
    ['Current session', resetLine],
    (line) => line.includes('Current session'),
    () => false
  )
}

describe('claude-pty reset parser time-zone handling', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    // 2026-07-15T10:00:00Z is Wednesday; 03:00 in LA (PDT), 06:00 in NY (EDT).
    vi.setSystemTime(new Date('2026-07-15T10:00:00.000Z'))
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('honors the IANA zone on a time-only reset instead of assuming local time', () => {
    const meta = parseReset('Resets 4am (America/Los_Angeles)')
    // 4:00am PDT on Jul 15 == 11:00Z; local-only handling would wrongly give NY 4am.
    expect(meta.resetsAt).toBe(Date.parse('2026-07-15T11:00:00.000Z'))
  })

  it('honors the IANA zone on a weekday reset instead of assuming local time', () => {
    const meta = parseReset('Resets Thursday at 4am (America/Los_Angeles)')
    // Next Thursday (Jul 16) 4:00am PDT == 11:00Z.
    expect(meta.resetsAt).toBe(Date.parse('2026-07-16T11:00:00.000Z'))
  })

  it('keeps local-time behavior when no zone is present', () => {
    // 9pm EDT (local) on Jul 15 == 01:00Z Jul 16.
    const meta = parseReset('Resets 9pm')
    expect(meta.resetsAt).toBe(Date.parse('2026-07-16T01:00:00.000Z'))
  })
})
