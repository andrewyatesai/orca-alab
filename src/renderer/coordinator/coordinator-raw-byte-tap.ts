// Raw PTY byte fan-out for the coordinator's focused aterm tile: the feed
// retains one bounded raw-ANSI tail per session (the grid previews' source); a
// focused tile taps that tail as its engine seed, then streams live chunks
// straight through — nothing extra is retained, the engine owns further history.

export type RawByteSink = (chunk: string) => void

export type SessionByteTaps = {
  /** Fan a live raw chunk out to every sink tapping `sessionId`. */
  deliver: (sessionId: string, chunk: string) => void
  /** Register a live-byte sink for `sessionId`; returns its untap disposer. */
  add: (sessionId: string, sink: RawByteSink) => () => void
}

export function createSessionByteTaps(): SessionByteTaps {
  const taps = new Map<string, Set<RawByteSink>>()
  return {
    deliver(sessionId, chunk) {
      const sinks = taps.get(sessionId)
      if (!sinks || chunk === '') {
        return
      }
      for (const sink of sinks) {
        sink(chunk)
      }
    },
    add(sessionId, sink) {
      const sinks = taps.get(sessionId) ?? new Set()
      sinks.add(sink)
      taps.set(sessionId, sinks)
      return () => {
        sinks.delete(sink)
        // Drop the empty set so idle sessions cost no map entry.
        if (sinks.size === 0) {
          taps.delete(sessionId)
        }
      }
    }
  }
}

/** The retained tail, made safe to replay into a fresh engine. A tail that hit
 *  its char bound was sliced mid-stream — possibly inside an escape sequence,
 *  whose remainder would print literally — so resync at the first line boundary
 *  (loses at most one visible line). An unbounded tail replays whole. */
export function seedForEngineReplay(tail: string, maxChars: number): string {
  if (tail.length < maxChars) {
    return tail
  }
  const firstNewline = tail.indexOf('\n')
  return firstNewline === -1 ? tail : tail.slice(firstNewline + 1)
}
