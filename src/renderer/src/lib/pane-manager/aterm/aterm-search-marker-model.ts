// Scrollbar search-marker model: map the sorted match list onto track fractions so
// the marker strip (and the worker STATE that mirrors it) can paint a tick per
// match region. Pure math — no DOM, no engine — so both the in-process wiring and
// the render worker derive markers from one implementation.

/** Marker positions as fractions of the whole retained buffer (0 = oldest line,
 *  1 = live bottom), deduped to at most one per bucket so a 50k-match query still
 *  yields a bounded paint + a bounded STATE payload. */
export type AtermSearchMarkerModel = {
  /** Sorted ascending, each in [0, 1]; at most SEARCH_MARKER_BUCKETS entries. */
  fractions: number[]
  /** Exact fraction of the active match, or null when none. */
  activeFraction: number | null
}

// Bucket count bounds the marker payload/paint. 256 ≈ sub-3px resolution on any
// realistic pane height — finer would paint marks the eye can't separate.
export const SEARCH_MARKER_BUCKETS = 256

const EMPTY_MODEL: AtermSearchMarkerModel = {
  fractions: [],
  activeFraction: null
}

/** Derive the marker model from matches sorted ascending by absolute line.
 *  `firstLine` is the oldest RETAINED absolute row (search_display_origin − base_y):
 *  ring eviction keeps absolute rows growing, so raw lines must be re-based or every
 *  marker in an evicted buffer would clamp to the bottom. `totalLines` is the retained
 *  line count (scrollback + viewport rows). Adjacent matches in one bucket collapse. */
export function computeSearchMarkerModel(
  matches: readonly { line: number }[],
  activeIndex: number,
  firstLine: number,
  totalLines: number
): AtermSearchMarkerModel {
  if (matches.length === 0 || totalLines <= 0) {
    return EMPTY_MODEL
  }
  const fractionFor = (line: number): number =>
    // +0.5 centers the marker on the line's band; clamp so an out-of-range line
    // (e.g. a stale match past a just-shrunk buffer) can't paint off-track.
    Math.min(1, Math.max(0, (line - firstLine + 0.5) / totalLines))
  const fractions: number[] = []
  let lastBucket = -1
  for (const match of matches) {
    const fraction = fractionFor(match.line)
    const bucket = Math.min(SEARCH_MARKER_BUCKETS - 1, Math.floor(fraction * SEARCH_MARKER_BUCKETS))
    if (bucket !== lastBucket) {
      fractions.push(fraction)
      lastBucket = bucket
    }
  }
  const active = activeIndex >= 0 && activeIndex < matches.length ? matches[activeIndex] : null
  return {
    fractions,
    activeFraction: active ? fractionFor(active.line) : null
  }
}

/** Last-result memo for the per-frame call sites (worker buildState, scrollbar
 *  refresh): the match list only changes identity on a re-index, so unchanged
 *  frames cost one reference compare instead of an O(matches) walk. */
export function createSearchMarkerModelCache(): typeof computeSearchMarkerModel {
  let lastMatches: readonly { line: number }[] | null = null
  let lastActive = -2
  let lastFirstLine = -1
  let lastTotal = -1
  let lastModel = EMPTY_MODEL
  return (matches, activeIndex, firstLine, totalLines) => {
    if (
      matches === lastMatches &&
      activeIndex === lastActive &&
      firstLine === lastFirstLine &&
      totalLines === lastTotal
    ) {
      return lastModel
    }
    lastMatches = matches
    lastActive = activeIndex
    lastFirstLine = firstLine
    lastTotal = totalLines
    lastModel = computeSearchMarkerModel(matches, activeIndex, firstLine, totalLines)
    return lastModel
  }
}

/** Cheap deep equality for repaint/notify gating (arrays are ≤ SEARCH_MARKER_BUCKETS). */
export function searchMarkerModelsEqual(
  a: AtermSearchMarkerModel,
  b: AtermSearchMarkerModel
): boolean {
  if (a === b) {
    return true
  }
  if (a.activeFraction !== b.activeFraction || a.fractions.length !== b.fractions.length) {
    return false
  }
  for (let i = 0; i < a.fractions.length; i++) {
    if (a.fractions[i] !== b.fractions[i]) {
      return false
    }
  }
  return true
}
