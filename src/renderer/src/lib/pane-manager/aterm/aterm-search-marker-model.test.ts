import { describe, expect, it } from 'vitest'
import {
  SEARCH_MARKER_BUCKETS,
  computeSearchMarkerModel,
  createSearchMarkerModelCache,
  searchMarkerModelsEqual
} from './aterm-search-marker-model'

const m = (lines: number[]): { line: number }[] => lines.map((line) => ({ line }))

describe('computeSearchMarkerModel', () => {
  it('returns the empty model for no matches or an empty buffer', () => {
    expect(computeSearchMarkerModel([], 0, 0, 100)).toEqual({
      fractions: [],
      activeFraction: null
    })
    expect(computeSearchMarkerModel(m([1]), 0, 0, 0)).toEqual({
      fractions: [],
      activeFraction: null
    })
  })

  it('maps lines to centered, sorted track fractions', () => {
    const model = computeSearchMarkerModel(m([0, 500, 999]), 1, 0, 1000)
    expect(model.fractions).toEqual([0.0005, 0.5005, 0.9995])
    expect(model.activeFraction).toBe(0.5005)
    // Sorted input stays sorted output (matches arrive line-ascending).
    expect([...model.fractions].sort((a, b) => a - b)).toEqual(model.fractions)
  })

  it('collapses same-bucket neighbors so the payload stays bounded', () => {
    // 50k dense matches (every line of a 50k buffer) must not emit 50k markers.
    const dense = m(Array.from({ length: 50_000 }, (_, i) => i))
    const model = computeSearchMarkerModel(dense, 0, 0, 50_000)
    expect(model.fractions.length).toBeLessThanOrEqual(SEARCH_MARKER_BUCKETS)
    expect(model.fractions.length).toBe(SEARCH_MARKER_BUCKETS)
  })

  it('re-bases absolute lines by the oldest retained row (ring eviction)', () => {
    // 10k lines evicted: absolute rows 10_000..10_999 span a 1000-line buffer.
    const model = computeSearchMarkerModel(m([10_000, 10_500]), -1, 10_000, 1000)
    expect(model.fractions).toEqual([0.0005, 0.5005])
    expect(model.activeFraction).toBeNull()
  })

  it('clamps a stale out-of-range line instead of painting off-track', () => {
    const model = computeSearchMarkerModel(m([5_000]), 0, 0, 100)
    expect(model.fractions).toEqual([1])
    expect(model.activeFraction).toBe(1)
  })

  it('ignores an out-of-range active index', () => {
    expect(computeSearchMarkerModel(m([1]), 5, 0, 100).activeFraction).toBeNull()
    expect(computeSearchMarkerModel(m([1]), -1, 0, 100).activeFraction).toBeNull()
  })
})

describe('createSearchMarkerModelCache', () => {
  it('returns the identical model while inputs are unchanged, recomputes on any change', () => {
    const cache = createSearchMarkerModelCache()
    const matches = m([10, 20])
    const first = cache(matches, 0, 0, 100)
    expect(cache(matches, 0, 0, 100)).toBe(first)
    // New match-list identity (a re-index) recomputes.
    expect(cache(m([10, 20]), 0, 0, 100)).not.toBe(first)
    // Geometry growth (streaming output) recomputes too.
    const grown = cache(matches, 0, 0, 200)
    expect(grown).not.toBe(first)
    expect(grown.fractions).toEqual([0.0525, 0.1025])
  })
})

describe('searchMarkerModelsEqual', () => {
  it('compares by value, not identity', () => {
    expect(
      searchMarkerModelsEqual(
        { fractions: [0.1, 0.2], activeFraction: 0.2 },
        { fractions: [0.1, 0.2], activeFraction: 0.2 }
      )
    ).toBe(true)
    expect(
      searchMarkerModelsEqual(
        { fractions: [0.1], activeFraction: null },
        { fractions: [0.1, 0.2], activeFraction: null }
      )
    ).toBe(false)
    expect(
      searchMarkerModelsEqual(
        { fractions: [0.1], activeFraction: 0.1 },
        { fractions: [0.1], activeFraction: null }
      )
    ).toBe(false)
    expect(
      searchMarkerModelsEqual(
        { fractions: [0.1], activeFraction: null },
        { fractions: [0.3], activeFraction: null }
      )
    ).toBe(false)
  })
})
