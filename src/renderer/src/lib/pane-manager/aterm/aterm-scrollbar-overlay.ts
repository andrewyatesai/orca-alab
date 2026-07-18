import {
  markTerminalPinnedViewport,
  syncTerminalScrollIntentFromViewport,
  type TerminalScrollIntentTarget
} from '../terminal-scroll-intent'
import type { AtermTerminal } from './aterm_wasm.js'

/** The engine slice the scrollbar reads/drives. Both the in-process engines and
 *  the worker-backed term expose it (snapshot-backed there, one frame stale). */
type ScrollbarEngine = Pick<
  AtermTerminal,
  'display_offset' | 'base_y' | 'is_alt_screen' | 'scroll_lines'
>

export type AtermScrollbarOverlayDeps = {
  term: ScrollbarEngine
  getRows: () => number
  redraw: () => void
  isDisposed: () => boolean
  /** The pane's scroll-intent target (facade). A thumb-drag scrolls the engine
   *  directly and the thumb carries no .xterm class, so dom-tracking's pointer gate
   *  never arms and the canvas emits no DOM scroll event — this is the only place
   *  the drag can record intent. Without it a keyed remount snaps to the bottom.
   *  Absent → no intent tracking (tests / pre-wire). */
  getScrollIntentTarget?: () => TerminalScrollIntentTarget | null
}

export type AtermScrollbarOverlay = {
  dispose: () => void
}

// Thumb geometry: 7px wide like the removed xterm scrollbar, but painted as an
// OVERLAY above the canvas (no reserved gutter column) — it appears only while
// scrolling/hovering, so it never permanently covers content (the concern that
// justified the old gutter, PR #5051).
const THUMB_WIDTH_PX = 7
const MIN_THUMB_HEIGHT_PX = 20
// Pointer within this many px of the pane's right edge counts as "hovering the
// scrollbar" and keeps it shown.
const HOVER_EDGE_PX = 14
const IDLE_HIDE_MS = 1000

/** A minimal VS-Code-like overlay scrollbar for an aterm pane: a thin thumb at
 *  the right edge sized/positioned from display_offset / total buffer lines,
 *  shown while wheel-scrolling or hovering the edge, faded out after idle, and
 *  hidden entirely on the alternate screen (TUIs own their own viewport).
 *  Dragging the thumb scrolls through the engine's scroll_lines. */
export function createAtermScrollbarOverlay(
  canvas: HTMLCanvasElement,
  deps: AtermScrollbarOverlayDeps
): AtermScrollbarOverlay {
  const { term, getRows, redraw, isDisposed, getScrollIntentTarget } = deps
  const host = canvas.parentElement

  // Record scroll intent on the facade after a thumb-driven scroll — the same seam
  // keyboard-handlers' Cmd+Up/Down and the wheel path use. mark-then-sync so a drag
  // that lands back at the bottom reclassifies to followOutput, and one that rests in
  // history keeps the pin, so a later keyed remount restores the reading position.
  const recordDragScrollIntent = (): void => {
    const intentTarget = getScrollIntentTarget?.()
    if (intentTarget) {
      markTerminalPinnedViewport(intentTarget)
      syncTerminalScrollIntentFromViewport(intentTarget)
    }
  }

  const thumb = document.createElement('div')
  thumb.dataset.testid = 'aterm-scrollbar-thumb' // e2e locator
  Object.assign(thumb.style, {
    position: 'absolute',
    right: '0',
    width: `${THUMB_WIDTH_PX}px`,
    // Above the canvas + search overlay, below nothing interactive (the IME
    // helpers box uses zIndex 5; match it so neither occludes the other's hits).
    zIndex: '5',
    // Same thumb recipe as .scrollbar-sleek in main.css (muted-foreground mixes).
    background: 'color-mix(in srgb, var(--muted-foreground, #737373) 28%, transparent)',
    opacity: '0',
    pointerEvents: 'none',
    transition: 'opacity 0.15s ease'
  } satisfies Partial<CSSStyleDeclaration>)
  host?.appendChild(thumb)

  let visible = false
  let hovering = false
  let dragging = false
  let hideTimer: ReturnType<typeof setTimeout> | null = null
  let rafId: number | null = null
  // Drag anchors + the offset we already asked the engine for: the worker-backed
  // display_offset lags a frame, so deltas are computed against this predicted
  // value or fast drags would compound the stale reads into over-scroll.
  let dragStartY = 0
  let dragStartThumbTop = 0
  let predictedOffset = 0

  const scrollableLines = (): number => (term.is_alt_screen ? 0 : term.base_y)

  const trackHeight = (): number => host?.clientHeight ?? canvas.clientHeight

  const thumbHeight = (): number => {
    const rows = Math.max(1, getRows())
    const total = scrollableLines() + rows
    return Math.max(MIN_THUMB_HEIGHT_PX, (trackHeight() * rows) / total)
  }

  const hideNow = (): void => {
    visible = false
    hovering = false
    thumb.style.opacity = '0'
    thumb.style.pointerEvents = 'none'
    if (rafId !== null) {
      cancelAnimationFrame(rafId)
      rafId = null
    }
  }

  // Recompute thumb size/position from the live engine state. Runs once per
  // frame while visible so scrolls from any source (wheel, keyboard, search)
  // keep the thumb honest without per-source hooks.
  const update = (): void => {
    rafId = null
    if (isDisposed()) {
      return
    }
    const lines = scrollableLines()
    if (lines <= 0) {
      hideNow()
      return
    }
    const height = thumbHeight()
    const range = Math.max(0, trackHeight() - height)
    const offset = dragging ? predictedOffset : term.display_offset
    const top = ((lines - offset) / lines) * range
    thumb.style.height = `${height}px`
    thumb.style.top = `${top}px`
    if (visible) {
      rafId = requestAnimationFrame(update)
    }
  }

  const armFade = (): void => {
    if (hideTimer !== null) {
      clearTimeout(hideTimer)
    }
    hideTimer = setTimeout(() => {
      hideTimer = null
      if (!dragging && !hovering) {
        hideNow()
      }
    }, IDLE_HIDE_MS)
  }

  const show = (): void => {
    if (isDisposed() || scrollableLines() <= 0) {
      return
    }
    if (!visible) {
      visible = true
      thumb.style.opacity = '1'
      thumb.style.pointerEvents = 'auto'
      update()
    }
    armFade()
  }

  const onHostWheel = (): void => {
    show()
  }

  const onHostMouseMove = (event: MouseEvent): void => {
    const rect = (host ?? canvas).getBoundingClientRect()
    const nearEdge = rect.right - event.clientX <= HOVER_EDGE_PX
    if (nearEdge) {
      hovering = true
      show()
    } else if (hovering) {
      hovering = false
      armFade()
    }
  }

  const onHostMouseLeave = (): void => {
    hovering = false
    armFade()
  }

  const onDragMove = (event: MouseEvent): void => {
    if (!dragging || isDisposed()) {
      return
    }
    const lines = scrollableLines()
    const range = Math.max(1, trackHeight() - thumbHeight())
    const top = Math.min(range, Math.max(0, dragStartThumbTop + (event.clientY - dragStartY)))
    const target = Math.round(lines - (top / range) * lines)
    const delta = target - predictedOffset
    if (delta !== 0) {
      predictedOffset = target
      term.scroll_lines(delta)
      redraw()
      recordDragScrollIntent()
    }
    event.preventDefault()
  }

  const onDragEnd = (): void => {
    if (!dragging) {
      return
    }
    dragging = false
    window.removeEventListener('mousemove', onDragMove)
    window.removeEventListener('mouseup', onDragEnd)
    // Settle the final intent at drag release (a last no-delta move records nothing,
    // and this is the moment the reading position is committed).
    recordDragScrollIntent()
    armFade()
  }

  const onThumbMouseDown = (event: MouseEvent): void => {
    if (event.button !== 0 || isDisposed()) {
      return
    }
    dragging = true
    dragStartY = event.clientY
    dragStartThumbTop = Number.parseFloat(thumb.style.top) || 0
    predictedOffset = term.display_offset
    window.addEventListener('mousemove', onDragMove)
    window.addEventListener('mouseup', onDragEnd)
    // Consume: a thumb grab must not start a text selection on the canvas.
    event.preventDefault()
    event.stopPropagation()
  }

  const onThumbWheel = (event: WheelEvent): void => {
    // The thumb sits above the canvas, so re-dispatch onto it: the single wheel
    // path (sensitivity, remainder carry) handles it instead of a dead strip.
    event.preventDefault()
    event.stopPropagation()
    canvas.dispatchEvent(new WheelEvent('wheel', event))
  }

  const hostTarget = host ?? canvas
  // Passive + non-capturing: visibility only; the scroll-input handler owns the
  // event (and hidden panes never fire these).
  hostTarget.addEventListener('wheel', onHostWheel, { passive: true })
  hostTarget.addEventListener('mousemove', onHostMouseMove)
  hostTarget.addEventListener('mouseleave', onHostMouseLeave)
  thumb.addEventListener('mousedown', onThumbMouseDown)
  thumb.addEventListener('wheel', onThumbWheel, { passive: false })

  return {
    dispose: () => {
      hostTarget.removeEventListener('wheel', onHostWheel)
      hostTarget.removeEventListener('mousemove', onHostMouseMove)
      hostTarget.removeEventListener('mouseleave', onHostMouseLeave)
      thumb.removeEventListener('mousedown', onThumbMouseDown)
      thumb.removeEventListener('wheel', onThumbWheel)
      onDragEnd()
      if (hideTimer !== null) {
        clearTimeout(hideTimer)
        hideTimer = null
      }
      if (rafId !== null) {
        cancelAnimationFrame(rafId)
        rafId = null
      }
      thumb.remove()
    }
  }
}
