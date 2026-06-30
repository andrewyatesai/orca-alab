// Serialized-buffer cache for the render worker: pushes a recent serialized buffer to
// the main thread so it can read scrollback SYNCHRONOUSLY at shutdown layout-capture.
// Throttle-with-max-wait, NOT a pure debounce: a continuously-busy pane would reset a
// debounce forever and never cache (then the shutdown read gets an empty/stale blob and
// the pane's scrollback is lost). Extracted to keep the worker entry focused.

type SerializedCacheMessage = { type: 'serializedCache'; full: string; scrollback: string }

type SerializeCacheDeps = {
  /** The live worker terminal (null before init / after dispose). */
  getTerm: () => { serializedCache: () => { full: string; scrollback: string } } | null
  post: (message: SerializedCacheMessage) => void
}

const DEBOUNCE_MS = 1000
const MAX_WAIT_MS = 5000

export function createWorkerSerializeCache(deps: SerializeCacheDeps): {
  /** Refresh ~1s after output settles, but at least every MAX_WAIT while output streams. */
  schedule: () => void
  /** Clear pending timers (call before terminating the worker). */
  dispose: () => void
} {
  let timer: ReturnType<typeof setTimeout> | null = null
  let maxWaitTimer: ReturnType<typeof setTimeout> | null = null

  const clearTimers = (): void => {
    if (timer !== null) {
      clearTimeout(timer)
      timer = null
    }
    if (maxWaitTimer !== null) {
      clearTimeout(maxWaitTimer)
      maxWaitTimer = null
    }
  }

  const flush = (): void => {
    clearTimers()
    const term = deps.getTerm()
    if (term) {
      const { full, scrollback } = term.serializedCache()
      deps.post({ type: 'serializedCache', full, scrollback })
    }
  }

  return {
    schedule: () => {
      // Debounce: refresh after output settles (the common idle case).
      if (timer !== null) {
        clearTimeout(timer)
      }
      timer = setTimeout(flush, DEBOUNCE_MS)
      // Max-wait floor: guarantee a refresh even while output streams continuously
      // (the debounce above would otherwise never fire).
      if (maxWaitTimer === null) {
        maxWaitTimer = setTimeout(flush, MAX_WAIT_MS)
      }
    },
    dispose: clearTimers
  }
}
