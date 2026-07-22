// Binary search over the SORTED search-match list for the on-screen slice. The
// engine emits matches oldest-to-newest (ascending absolute line), and the overlay
// paths run per frame — a linear scan of ALL matches (possibly 100k) per frame was
// the P6 hot spot; two lower-bound probes make it O(log n + visible).

/** The minimal match shape the range probe needs (both the worker's WorkerMatch
 *  and the main thread's AtermSearchMatch satisfy it). */
export type LineOrderedMatch = { line: number }

// First index whose match line >= `line` (classic lower bound; matches.length when none).
function lowerBoundByLine(matches: readonly LineOrderedMatch[], line: number): number {
  let lo = 0
  let hi = matches.length
  while (lo < hi) {
    const mid = (lo + hi) >>> 1
    if (matches[mid].line < line) {
      lo = mid + 1
    } else {
      hi = mid
    }
  }
  return lo
}

/** Index range `[start, end)` of the matches whose absolute line falls inside
 *  `[firstLine, endLine)` — the visible viewport band. Requires `matches` sorted
 *  ascending by `line` (the engine's emission order). Empty range when nothing
 *  is on screen. */
export function visibleMatchRange(
  matches: readonly LineOrderedMatch[],
  firstLine: number,
  endLine: number
): { start: number; end: number } {
  if (matches.length === 0 || endLine <= firstLine) {
    return { start: 0, end: 0 }
  }
  return { start: lowerBoundByLine(matches, firstLine), end: lowerBoundByLine(matches, endLine) }
}
