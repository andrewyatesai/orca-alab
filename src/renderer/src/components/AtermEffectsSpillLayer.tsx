import { useLayoutEffect, useRef, useSyncExternalStore } from 'react'
import { atermSpillOverlay } from '@/lib/pane-manager/aterm/aterm-spill-overlay'
import { startAtermSpillGeometryTracker } from '@/lib/pane-manager/aterm/aterm-spill-geometry'

/** Window-space cross-pane effects spill overlay (stage 2 scaffold). Renders
 *  null until a spill-export-capable engine registers a pane — no engine sets
 *  that capability yet, so today this mounts nothing and costs nothing. Once
 *  panes register, the canvas spans the terminal-surfaces container and the
 *  geometry tracker + compositor own everything drawn on it. */
export default function AtermEffectsSpillLayer(): React.JSX.Element | null {
  const hasSpillPanes = useSyncExternalStore(
    atermSpillOverlay.subscribe,
    () => atermSpillOverlay.getPaneCount() > 0
  )
  const canvasRef = useRef<HTMLCanvasElement | null>(null)

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

  if (!hasSpillPanes) {
    return null
  }
  return (
    <canvas
      ref={canvasRef}
      data-testid="aterm-effects-spill-overlay"
      aria-hidden="true"
      // Why: spill pixels are purely decorative — hit-testing, drags and wheel
      // events must pass through to the panes below at all times.
      className="absolute inset-0 pointer-events-none z-[var(--z-terminal-spill)]"
    />
  )
}
