import { useEffect, useRef, useState } from 'react'

/** Rate-limits a rapidly-changing value to at most one emit per `intervalMs`
 *  while always surfacing the latest value. OpenCode publishes a streaming
 *  assistant frame per part; unthrottled, each re-parses the whole bubble.
 *
 *  `resetKey` identifies the value's source (e.g. the chat session). When it
 *  changes, the held/trailing value from the prior source is dropped and the new
 *  value emitted at once, so a fast switch cannot bleed the old source's last
 *  frame over the new one. */
export function useThrottledLatestValue<T>(value: T, intervalMs: number, resetKey?: unknown): T {
  const [throttled, setThrottled] = useState(value)
  const valueRef = useRef(value)
  valueRef.current = value
  const lastEmitRef = useRef(0)
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const resetKeyRef = useRef(resetKey)
  const [renderKey, setRenderKey] = useState(resetKey)

  // Reset BEFORE paint: correct the returned value during render on a source
  // switch (React's adjust-state-during-render), so the newly-keyed view can't
  // paint the prior source's throttled frame for one frame while the effect
  // below catches up. Ref/timer teardown stays in the effect (a render may be
  // discarded, so side effects don't belong here).
  if (renderKey !== resetKey) {
    setRenderKey(resetKey)
    setThrottled(value)
  }

  useEffect(() => {
    if (resetKeyRef.current !== resetKey) {
      // Source changed: drop any trailing emit from the prior source and show the
      // new source's current value immediately (even when non-null, which the
      // interval path below would otherwise defer).
      resetKeyRef.current = resetKey
      if (timerRef.current) {
        clearTimeout(timerRef.current)
        timerRef.current = null
      }
      lastEmitRef.current = 0
      setThrottled(value)
      return
    }
    if (value == null) {
      // Turn ended: drop any trailing emit and reset so the next stream's first
      // frame shows at once instead of the stale bubble lingering.
      if (timerRef.current) {
        clearTimeout(timerRef.current)
        timerRef.current = null
      }
      lastEmitRef.current = 0
      setThrottled(value)
      return
    }
    const elapsed = Date.now() - lastEmitRef.current
    if (elapsed >= intervalMs) {
      lastEmitRef.current = Date.now()
      setThrottled(value)
      return
    }
    if (timerRef.current) {
      return
    }
    timerRef.current = setTimeout(() => {
      timerRef.current = null
      lastEmitRef.current = Date.now()
      setThrottled(valueRef.current)
    }, intervalMs - elapsed)
  }, [value, intervalMs, resetKey])

  useEffect(
    () => () => {
      if (timerRef.current) {
        clearTimeout(timerRef.current)
        timerRef.current = null
      }
    },
    []
  )

  return throttled
}
