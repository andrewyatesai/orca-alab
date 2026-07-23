// Exercises the REAL napi addon: the runtime emulator's search surface must
// return STABLE host rows (fed §2.4) that keep identifying the same line
// across eviction, and the context window must accept those stable rows.
import { describe, expect, it } from 'vitest'
import { HeadlessEmulator } from './headless-emulator'

function emulatorWithLines(lines: string[], opts: { rows?: number; scrollback?: number } = {}) {
  const emulator = new HeadlessEmulator({
    cols: 40,
    rows: opts.rows ?? 2,
    scrollback: opts.scrollback ?? 1000
  })
  for (const line of lines) {
    emulator.writeSync(`${line}\r\n`)
  }
  return emulator
}

describe('HeadlessEmulator.searchScrollback', () => {
  it('finds matches with stable host rows and honest totals', () => {
    const emulator = emulatorWithLines(['alpha needle', 'plain', 'beta NEEDLE'])
    const outcome = emulator.searchScrollback({ query: 'needle' })
    expect(outcome).not.toBeNull()
    expect(outcome?.total).toBe(2)
    expect(outcome?.incomplete).toBe(false)
    expect(outcome?.originRow).toBe(0)
    // Newest first, rows are origin-based (origin 0 here → retained order).
    expect(outcome?.matches.map((m) => m.line)).toEqual(['beta NEEDLE', 'alpha needle'])
    expect(outcome?.matches[0].hostRow).toBeGreaterThan(outcome!.matches[1].hostRow)
    emulator.dispose()
  })

  it('keeps a match at the SAME stable host row while eviction advances the origin', () => {
    const emulator = emulatorWithLines(
      Array.from({ length: 6 }, (_, i) => `row ${i}`),
      { rows: 1, scrollback: 6 }
    )
    const before = emulator.searchScrollback({ query: 'row 4' })
    expect(before?.total).toBe(1)
    const stableRow = before!.matches[0].hostRow
    emulator.writeSync('row 6\r\nrow 7\r\n')
    const after = emulator.searchScrollback({ query: 'row 4' })
    expect(after?.total).toBe(1)
    expect(after?.originRow).toBeGreaterThan(0)
    expect(after?.matches[0].hostRow).toBe(stableRow)
    emulator.dispose()
  })

  it('flags incomplete when the cap truncates and treats invalid regex as zero matches', () => {
    const emulator = emulatorWithLines(['x 1', 'x 2', 'x 3'])
    const capped = emulator.searchScrollback({ query: 'x', maxMatches: 1 })
    expect(capped?.matches).toHaveLength(1)
    expect(capped?.total).toBe(3)
    expect(capped?.incomplete).toBe(true)
    const bad = emulator.searchScrollback({ query: '(unclosed', regex: true })
    expect(bad).toMatchObject({ matches: [], total: 0, incomplete: false })
    emulator.dispose()
  })
})

describe('HeadlessEmulator.searchContext', () => {
  it('returns the window around a stable host row', () => {
    const emulator = emulatorWithLines(['one', 'two', 'three', 'four'])
    const match = emulator.searchScrollback({ query: 'three' })!.matches[0]
    const window = emulator.searchContext(match.hostRow, 1, 1)
    expect(window?.lines).toEqual(['two', 'three', 'four'])
    expect(window?.firstHostRow).toBe(match.hostRow - 1)
    emulator.dispose()
  })

  it('answers empty for a stable row older than the retained origin (evicted)', () => {
    const emulator = emulatorWithLines(
      Array.from({ length: 12 }, (_, i) => `row ${i}`),
      { rows: 1, scrollback: 4 }
    )
    // Origin has advanced past the earliest rows; host row 0 is gone.
    expect(emulator.retainedOriginRow()).toBeGreaterThan(0)
    const window = emulator.searchContext(0, 2, 2)
    expect(window?.lines).toEqual([])
    emulator.dispose()
  })
})
