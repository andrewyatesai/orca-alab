import { describe, expect, it } from 'vitest'
import { deriveSessionStatus, formatTimeSinceActivity, orderAttentionQueue } from './session-status'

describe('deriveSessionStatus', () => {
  it('reports working while a non-shell process owns the PTY foreground', () => {
    expect(
      deriveSessionStatus({ isAlive: true, exitCode: null, foregroundProcess: 'claude' })
    ).toBe('working')
    expect(deriveSessionStatus({ isAlive: true, exitCode: null, foregroundProcess: 'cargo' })).toBe(
      'working'
    )
  })

  it('reports working when the foreground process is unknown (no inference)', () => {
    expect(deriveSessionStatus({ isAlive: true, exitCode: null, foregroundProcess: null })).toBe(
      'working'
    )
  })

  it('reports needs-you when the shell itself sits at the foreground (a prompt)', () => {
    expect(deriveSessionStatus({ isAlive: true, exitCode: null, foregroundProcess: 'zsh' })).toBe(
      'needs-you'
    )
    expect(
      deriveSessionStatus({ isAlive: true, exitCode: null, foregroundProcess: 'bash.exe' })
    ).toBe('needs-you')
  })

  it('reports done on a clean exit and failed on a non-zero one', () => {
    expect(deriveSessionStatus({ isAlive: false, exitCode: 0, foregroundProcess: null })).toBe(
      'done'
    )
    expect(deriveSessionStatus({ isAlive: false, exitCode: 1, foregroundProcess: null })).toBe(
      'failed'
    )
    expect(deriveSessionStatus({ isAlive: false, exitCode: -1, foregroundProcess: null })).toBe(
      'failed'
    )
  })

  it('reports ended (unknown outcome, never done) for a vanished session with no exit event', () => {
    // The #9263 shape: reaped during a stream disconnect / daemon restart —
    // exitCode stays null, so there is no completion evidence either.
    expect(deriveSessionStatus({ isAlive: false, exitCode: null, foregroundProcess: 'zsh' })).toBe(
      'ended'
    )
    expect(deriveSessionStatus({ isAlive: false, exitCode: null, foregroundProcess: null })).toBe(
      'ended'
    )
  })
})

describe('formatTimeSinceActivity', () => {
  const base = 1_700_000_000_000
  it('collapses very recent activity to "now"', () => {
    expect(formatTimeSinceActivity(base + 3000, base)).toBe('now')
  })

  it('scales through seconds, minutes, hours, and days', () => {
    expect(formatTimeSinceActivity(base + 42_000, base)).toBe('42s')
    expect(formatTimeSinceActivity(base + 5 * 60_000, base)).toBe('5m')
    expect(formatTimeSinceActivity(base + 3 * 3_600_000, base)).toBe('3h')
    expect(formatTimeSinceActivity(base + 2 * 86_400_000, base)).toBe('2d')
  })

  it('never goes negative on clock skew', () => {
    expect(formatTimeSinceActivity(base, base + 10_000)).toBe('now')
  })
})

describe('orderAttentionQueue', () => {
  it('excludes working sessions and puts needs-you before finished, newest first', () => {
    const queue = orderAttentionQueue([
      { id: 'w', status: 'working', lastActivityAt: 500 } as const,
      { id: 'd-old', status: 'done', lastActivityAt: 100 } as const,
      { id: 'n-old', status: 'needs-you', lastActivityAt: 200 } as const,
      { id: 'f-new', status: 'failed', lastActivityAt: 400 } as const,
      { id: 'n-new', status: 'needs-you', lastActivityAt: 300 } as const
    ])
    expect(queue.map((entry) => entry.id)).toEqual(['n-new', 'n-old', 'f-new', 'd-old'])
  })

  it('keeps ended (unknown outcome) sessions in the finished band', () => {
    const queue = orderAttentionQueue([
      { id: 'e', status: 'ended', lastActivityAt: 250 } as const,
      { id: 'n', status: 'needs-you', lastActivityAt: 100 } as const,
      { id: 'd', status: 'done', lastActivityAt: 300 } as const
    ])
    expect(queue.map((entry) => entry.id)).toEqual(['n', 'd', 'e'])
  })
})
