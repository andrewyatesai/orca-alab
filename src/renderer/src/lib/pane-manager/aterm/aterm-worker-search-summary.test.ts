import { describe, expect, it, vi } from 'vitest'
import {
  createAtermSearchSummaryReader,
  hasAtermSearchSummary
} from './aterm-worker-search-summary'

const summary = (
  matches: { absRow: number; col: number; len: number; snippet: string }[],
  total = matches.length,
  incomplete = false
): string => JSON.stringify({ matches, total, incomplete })

describe('hasAtermSearchSummary', () => {
  it('is false for pre-E-1 engines (no export) and non-function members', () => {
    expect(hasAtermSearchSummary({})).toBe(false)
    expect(hasAtermSearchSummary({ search_summary: 'not-a-fn' })).toBe(false)
  })

  it('is true when the pinned artifact exposes the export', () => {
    expect(hasAtermSearchSummary({ search_summary: () => '{"matches":[],"total":0,"incomplete":false}' })).toBe(true)
  })
})

describe('createAtermSearchSummaryReader.read', () => {
  it('returns null (count-only degradation) when the engine lacks the export', () => {
    const reader = createAtermSearchSummaryReader({})
    expect(reader.read('q', false, false, 50)).toBeNull()
  })

  it('parses a valid summary, forwarding query/options/cap to the export', () => {
    const search_summary = vi.fn(() =>
      summary([{ absRow: 9, col: 2, len: 3, snippet: 'aa[bbb]cc' }], 4, true)
    )
    const reader = createAtermSearchSummaryReader({ search_summary })
    const result = reader.read('bbb', true, false, 50)
    expect(search_summary).toHaveBeenCalledExactlyOnceWith('bbb', true, false, 50)
    expect(result).toEqual({
      matches: [{ absRow: 9, col: 2, len: 3, snippet: 'aa[bbb]cc' }],
      total: 4,
      incomplete: true
    })
  })

  it('treats undefined as transiently-unavailable: null now, retries next call', () => {
    const search_summary = vi
      .fn<(q: string, cs: boolean, re: boolean, max: number) => string | undefined>()
      .mockReturnValueOnce(undefined)
      .mockReturnValueOnce(summary([{ absRow: 1, col: 0, len: 1, snippet: '[a]' }]))
    const reader = createAtermSearchSummaryReader({ search_summary })
    expect(reader.read('a', false, false, 50)).toBeNull()
    expect(reader.read('a', false, false, 50)?.matches[0].snippet).toBe('[a]')
    expect(search_summary).toHaveBeenCalledTimes(2)
  })

  it.each([
    ['malformed JSON', 'not json'],
    ['non-object payload', '[]'],
    ['missing matches array', '{"total":0,"incomplete":false}'],
    ['missing total', JSON.stringify({ matches: [], incomplete: false })],
    ['non-boolean incomplete', JSON.stringify({ matches: [], total: 0, incomplete: 'no' })],
    [
      'record missing snippet',
      JSON.stringify({ matches: [{ absRow: 1, col: 0, len: 1 }], total: 1, incomplete: false })
    ],
    [
      'record non-numeric absRow',
      JSON.stringify({
        matches: [{ absRow: 'x', col: 0, len: 1, snippet: 'a' }],
        total: 1,
        incomplete: false
      })
    ]
  ])('fails closed on %s: null now AND the reader is disabled for good', (_name, payload) => {
    const search_summary = vi.fn(() => payload)
    const reader = createAtermSearchSummaryReader({ search_summary })
    expect(reader.read('q', false, false, 50)).toBeNull()
    // Skew is permanent for the pinned artifact — never probe the export again.
    expect(reader.read('q', false, false, 50)).toBeNull()
    expect(search_summary).toHaveBeenCalledTimes(1)
  })
})

describe('createAtermSearchSummaryReader.snippetsByRow', () => {
  it('maps absRow → snippet for enriching already-found match rows', () => {
    const search_summary = vi.fn(() =>
      summary([
        { absRow: 5, col: 0, len: 1, snippet: '[a]' },
        { absRow: 9, col: 1, len: 1, snippet: 'x[a]' }
      ])
    )
    const map = createAtermSearchSummaryReader({ search_summary }).snippetsByRow('a', false, false, 50)
    expect(map?.get(5)).toBe('[a]')
    expect(map?.get(9)).toBe('x[a]')
    expect(map?.has(7)).toBe(false)
  })

  it('is null when the export is absent (matches keep null snippets)', () => {
    expect(createAtermSearchSummaryReader({}).snippetsByRow('a', false, false, 50)).toBeNull()
  })
})
