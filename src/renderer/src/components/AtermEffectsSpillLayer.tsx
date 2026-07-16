import { useLayoutEffect, useRef, useSyncExternalStore } from 'react'
import { atermSpillOverlay } from '@/lib/pane-manager/aterm/aterm-spill-overlay'
import { atermSpillWorkerBridge } from '@/lib/pane-manager/aterm/aterm-spill-worker-bridge'
import { startAtermSpillGeometryTracker } from '@/lib/pane-manager/aterm/aterm-spill-geometry'

/** Window-space cross-pane effects spill overlay. Renders null until a
 *  spill-export-capable engine registers a pane; then the canvases span the
 *  terminal-surfaces container and the geometry tracker + compositors own
 *  everything drawn on them. TWO sibling canvases, one per render path: the
 *  main-thread 2d canvas for in-process panes, and a transferred-to-the-shared-
 *  render-worker canvas for worker panes (mounted only while worker panes are
 *  bound, and KEYED by the bridge's canvas generation — a transfer is
 *  irreversible per element, so each worker respawn/rebind remounts a fresh
 *  element that the bridge transfers under a higher epoch). */
export default function AtermEffectsSpillLayer(): React.JSX.Element | null {
  const hasSpillPanes = useSyncExternalStore(
    atermSpillOverlay.subscribe,
    () => atermSpillOverlay.getPaneCount() > 0
  )
  const hasWorkerPanes = useSyncExternalStore(
    atermSpillWorkerBridge.subscribe,
    atermSpillWorkerBridge.hasWorkerPanes
  )
  const workerGeneration = useSyncExternalStore(
    atermSpillWorkerBridge.subscribe,
    atermSpillWorkerBridge.getCanvasGeneration
  )
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const workerCanvasRef = useRef<HTMLCanvasElement | null>(null)

  useLayoutEffect(() => {
    const canvas = canvasRef.current
    const container = canvas?.parentElement
    if (!hasSpillPanes || !canvas || !container) {
      return undefined
    }
    const detachCanvas = atermSpillOverlay.attachCanvas(canvas)
    const tracker = startAtermSpillGeometryTracker({ container })
    return () => {
      tracker.dispose()
      detachCanvas()
    }
  }, [hasSpillPanes])

  useLayoutEffect(() => {
    const canvas = workerCanvasRef.current
    if (!hasWorkerPanes || !canvas) {
      return
    }
    // Transfer + init are idempotent per element (StrictMode-safe); the bridge
    // owns release, so unmount needs no cleanup here.
    atermSpillWorkerBridge.attachWorkerCanvas(canvas)
  }, [hasWorkerPanes, workerGeneration])

  if (!hasSpillPanes) {
    return null
  }
  return (
    <>
      <canvas
        ref={canvasRef}
        data-testid="aterm-effects-spill-overlay"
        aria-hidden="true"
        // Why: spill pixels are purely decorative — hit-testing, drags and wheel
        // events must pass through to the panes below at all times.
        className="absolute inset-0 pointer-events-none z-[var(--z-terminal-spill)]"
      />
      {hasWorkerPanes ? (
        <canvas
          key={workerGeneration}
          ref={workerCanvasRef}
          data-testid="aterm-effects-spill-overlay-worker"
          aria-hidden="true"
          className="absolute inset-0 pointer-events-none z-[var(--z-terminal-spill)]"
        />
      ) : null}
    </>
  )
}
