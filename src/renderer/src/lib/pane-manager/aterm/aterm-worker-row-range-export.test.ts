import { describe, expect, it, vi } from 'vitest'
import {
  createAtermRowRangeReader,
  hasAtermRowRangeExport
} from './aterm-worker-row-range-export'

const record = (text: string, widths?: string) => ({
  text,
  wrapped: false,
  len: text.length,
  ...(widths === undefined ? {} : { widths })
})

describe('hasAtermRowRangeExport', () => {
  it('is false for pre-E9 engines (no export) and non-function members', () => {
    expect(hasAtermRowRangeExport({})).toBe(false)
    expect(hasAtermRowRangeExport({ row_range_json: 'not-a-fn' })).toBe(false)
  })

  it('is true when the pinned artifact exposes the export', () => {
    expect(hasAtermRowRangeExport({ row_range_json: () => '[]' })).toBe(true)
  })
})

describe('createAtermRowRangeReader', () => {
  it('returns null (per-row fallback) when the engine lacks the export', () => {
    const reader = createAtermRowRangeReader({})
    expect(reader.read(0, 2, 4)).toBeNull()
  })

  it('parses a valid payload in one engine call, widths optional', () => {
    const row_range_json = vi.fn(() => JSON.stringify([record('ab'), record('c漢', '121')]))
    const reader = createAtermRowRangeReader({ row_range_json })
    const rows = reader.read(0, 2, 3)
    expect(row_range_json).toHaveBeenCalledExactlyOnceWith(0, 2)
    expect(rows).toEqual([
      { text: 'ab', wrapped: false, len: 2 },
      { text: 'c漢', wrapped: false, len: 2, widths: '121' }
    ])
  })

  it('treats undefined as range-unavailable: falls back this read, retries the next', () => {
    const row_range_json = vi
      .fn<(first: number, count: number) => string | undefined>()
      .mockReturnValueOnce(undefined)
      .mockReturnValueOnce(JSON.stringify([record('a')]))
    const reader = createAtermRowRangeReader({ row_range_json })
    expect(reader.read(0, 1, 4)).toBeNull()
    expect(reader.read(0, 1, 4)).toEqual([{ text: 'a', wrapped: false, len: 1 }])
    expect(row_range_json).toHaveBeenCalledTimes(2)
  })

  it.each([
    ['malformed JSON', 'not json'],
    ['non-array payload', '{"rows":[]}'],
    ['row-count mismatch', JSON.stringify([record('a')])],
    ['non-object record', JSON.stringify([record('a'), 7])],
    ['missing text', JSON.stringify([record('a'), { wrapped: false, len: 0 }])],
    [
      'widths length != cols',
      JSON.stringify([record('a'), record('b', '11')]) // cols is 4 below
    ]
  ])('fails closed on %s: null now AND the batch path is disabled for good', (_name, payload) => {
    const row_range_json = vi.fn(() => payload)
    const reader = createAtermRowRangeReader({ row_range_json })
    expect(reader.read(0, 2, 4)).toBeNull()
    // Skew is permanent for the pinned artifact — never probe the export again.
    expect(reader.read(0, 2, 4)).toBeNull()
    expect(row_range_json).toHaveBeenCalledTimes(1)
  })
})
