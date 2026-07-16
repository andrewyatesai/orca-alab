// Read-only aterm tile for the coordinator's focused view (the coordinator-v0
// design's named rendering follow-up): one in-process CPU engine per visible
// tile, fed the subscriber's raw byte stream. No input wiring — v0 safety
// default; the parent keeps the text mirror as the load/failure fallback.
import { useEffect, useRef } from 'react'
import {
  createAtermPaneController,
  type AtermPaneController
} from '@/lib/pane-manager/aterm/aterm-pane-renderer'
import { tapCoordinatorSessionBytes } from './coordinator-session-feed'

// v0 render policy (design doc): in-process CPU drawer. These are the documented
// strategy opt-outs, read at controller creation; the shared render worker and
// GPU paths are the many-tile follow-up. Window-scoped, so they only affect the
// coordinator window — the main app window keeps its own defaults.
function forceInProcessCpuStrategy(): void {
  window.__atermWorkerRender = false
  window.__atermGpuDisabled = true
}

// The subscriber is a display-only follower: keystrokes, engine auto-replies
// (DA/CPR) and grid resizes must never reach the owner's PTY.
const dropPtyBytes = (): undefined => undefined

export function FocusedSessionTerminal({
  sessionId,
  onReady,
  onFailed
}: {
  sessionId: string
  /** First frame is up — the parent can drop its text-mirror overlay. */
  onReady: () => void
  /** Engine failed to load — the parent falls back to the text mirror. */
  onFailed: () => void
}): React.JSX.Element {
  const hostRef = useRef<HTMLDivElement | null>(null)
  // Callbacks live in a ref so a parent re-render (new closures) never tears
  // down and rebuilds the engine — only a sessionId change does.
  const callbacksRef = useRef({ onReady, onFailed })
  callbacksRef.current = { onReady, onFailed }

  useEffect(() => {
    const host = hostRef.current
    if (!host) {
      return
    }
    forceInProcessCpuStrategy()
    let cancelled = false
    let controller: AtermPaneController | null = null
    let untap: (() => void) | null = null
    createAtermPaneController(host, dropPtyBytes, dropPtyBytes, dropPtyBytes, undefined, {
      // Steady cursor: a follower tile shouldn't run a blink timer.
      getCursorBlink: () => false
    })
      .then((created) => {
        // Unfocused/unmounted while wasm + font loaded: free the engine now
        // (the shared-worker memory lesson — dispose every engine you create).
        if (cancelled) {
          created.dispose()
          return
        }
        controller = created
        // Seed (bounded retained tail) then live raw chunks, in stream order.
        untap = tapCoordinatorSessionBytes(sessionId, (chunk) => created.process(chunk))
        created.scheduleDraw()
        callbacksRef.current.onReady()
      })
      .catch((error: unknown) => {
        console.error('[coordinator] aterm tile failed to load; using the text mirror', error)
        if (!cancelled) {
          callbacksRef.current.onFailed()
        }
      })
    return () => {
      cancelled = true
      untap?.()
      untap = null
      controller?.dispose()
      controller = null
    }
  }, [sessionId])

  return <div ref={hostRef} className="absolute inset-0" />
}
