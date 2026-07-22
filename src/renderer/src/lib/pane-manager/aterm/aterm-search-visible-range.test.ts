import { describe, expect, it } from 'vitest'
import { visibleMatchRange, type LineOrderedMatch } from './aterm-search-visible-range'

const lines = (...ns: number[]): LineOrderedMatch[] => ns.map((line) => ({ line }))

// The probe must agree byte-for-byte with the linear scan it replaced: a match is
// visible iff firstLine <= line < endLine (the overlay's display-row band).
function linearRange(
  matches: LineOrderedMatch[],
  firstLine: number,
  endLine: number
): { start: number; end: number } {
  let start = matches.length
  let end = 0
  for (let i = 0; i < matches.length; i++) {
    if (matches[i].line >= firstLine && matches[i].line < endLine) {
      start = Math.min(start, i)
      end = Math.max(end, i + 1)
    }
  }
  return start >= end ? { start: 0, end: 0 } : { start, end }
}

describe('visibleMatchRange', () => {
  it('returns the empty range for no matches or an empty band', () => {
    expect(visibleMatchRange([], 0, 24)).toEqual({ start: 0, end: 0 })
    expect(visibleMatchRange(lines(1, 2, 3), 5, 5)).toEqual({ start: 0, end: 0 })
    expect(visibleMatchRange(lines(1, 2, 3), 5, 3)).toEqual({ start: 0, end: 0 })
  })

  it('brackets exactly the on-screen band (inclusive first, exclusive end)', () => {
    const matches = lines(0, 5, 10, 10, 11, 24, 25)
    const { start, end } = visibleMatchRange(matches, 10, 25)
    expect(start).toBe(2) // first line >= 10 (both duplicates included)
    expect(end).toBe(6) // line 25 excluded (endLine is exclusive)
  })

  it('handles bands entirely before/after all matches', () => {
    const matches = lines(100, 200, 300)
    expect(visibleMatchRange(matches, 0, 50)).toEqual({ start: 0, end: 0 })
    const after = visibleMatchRange(matches, 400, 500)
    expect(after.start).toBe(after.end) // empty (both probes land past the end)
  })

  it('tolerates a negative firstLine (viewport above the oldest match)', () => {
    const matches = lines(0, 1, 2)
    expect(visibleMatchRange(matches, -5, 2)).toEqual({ start: 0, end: 2 })
  })

  it('matches the linear scan it replaced across a sweep of viewports', () => {
    const matches = lines(0, 0, 3, 7, 7, 7, 12, 20, 21, 40, 41, 42, 90)
    for (let first = -2; first < 95; first += 3) {
      for (const rows of [1, 5, 24]) {
        const probe = visibleMatchRange(matches, first, first + rows)
        const linear = linearRange(matches, first, first + rows)
        // Same visible SLICE always; identical indices whenever the band is non-empty
        // (an empty probe may land anywhere — only its emptiness is observable).
        expect(matches.slice(probe.start, probe.end)).toEqual(
          matches.slice(linear.start, linear.end)
        )
        if (linear.end > linear.start) {
          expect(probe).toEqual(linear)
        }
      }
    }
  })
})
