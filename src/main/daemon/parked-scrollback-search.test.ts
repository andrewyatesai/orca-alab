// Exercises the REAL napi addon (like headless-emulator tests): the parked
// in-process search must share the daemon kernel's semantics.
import { describe, expect, it } from 'vitest'
import {
  searchStoredScrollback,
  storedScrollbackSearchContext
} from './parked-scrollback-search'

const content = {
  cols: 40,
  rows: 4,
  chunks: [
    // Stored ANSI: colored + plain lines, as a parked snapshot would hold.
    '\x1b[1;32mgreen needle\x1b[0m\r\n',
    'plain line\r\nanother NEEDLE row\r\ntail'
  ]
}

describe('searchStoredScrollback', () => {
  it('replays stored ANSI through the native engine and matches plain text', () => {
    const outcome = searchStoredScrollback(content, { query: 'needle' })
    expect(outcome).not.toBeNull()
    expect(outcome?.total).toBe(2)
    expect(outcome?.incomplete).toBe(false)
    // Newest first; ANSI stripped by the headless parse.
    expect(outcome?.matches[0].line).toBe('another NEEDLE row')
    expect(outcome?.matches[1].line).toBe('green needle')
  })

  it('honors case sensitivity and the match cap', () => {
    const sensitive = searchStoredScrollback(content, { query: 'NEEDLE', caseSensitive: true })
    expect(sensitive?.total).toBe(1)
    const capped = searchStoredScrollback(content, { query: 'needle', maxMatches: 1 })
    expect(capped?.matches).toHaveLength(1)
    expect(capped?.total).toBe(2)
    expect(capped?.incomplete).toBe(true)
  })

  it('treats an invalid regex as zero matches (find-bar parity)', () => {
    const outcome = searchStoredScrollback(content, { query: '(unclosed', regex: true })
    expect(outcome).toEqual({ matches: [], total: 0, incomplete: false })
  })
})

describe('storedScrollbackSearchContext', () => {
  it('returns the clamped window around a matched row', () => {
    const outcome = searchStoredScrollback(content, { query: 'plain line' })
    const absRow = outcome!.matches[0].absRow
    const window = storedScrollbackSearchContext(content, { absRow, before: 1, after: 1 })
    expect(window?.lines).toEqual(['green needle', 'plain line', 'another NEEDLE row'])
    expect(window?.firstAbsRow).toBe(absRow - 1)
  })
})
