import { ATERM_TEXTAREA_PARKED_STYLE } from './aterm-input-dom'
import type { AtermTerminal } from './aterm_wasm.js'

/** Live cursor-anchored IME view for a composition (compositionstart..end):
 *  the helper textarea is moved to the CURSOR CELL so the OS candidate window
 *  opens at the caret (parked off-screen, it opened at the pane corner), and the
 *  in-progress preedit text is painted at that cell on a stacked overlay canvas
 *  (the engine never sees uncommitted text, so without this the user types
 *  blind until the commit). */
export type AtermCompositionView = {
  /** compositionstart: anchor the textarea to the cursor cell. */
  begin: () => void
  /** compositionupdate: re-anchor + paint the preedit text at the cursor. */
  update: (preedit: string) => void
  /** compositionend/cancel: clear the overlay + re-park the textarea. */
  end: () => void
  dispose: () => void
}

export type AtermCompositionViewDeps = {
  /** The grid canvas — the overlay stacks exactly over it (same parent). */
  canvas: HTMLCanvasElement
  /** The helper textarea this view anchors/parks. */
  textarea: HTMLTextAreaElement
  term: AtermTerminal
  /** Live cell metrics in device px + dpr (mutated in place on DPI change). */
  metrics: { dpr: number; cellWidth: number; cellHeight: number }
  /** Live theme (mutated in place on re-theme) — preedit fg/bg follow it. */
  themeColors: { fg: number; bg: number }
  /** Preferred anchor cell when the app parks the real cursor away from its
   *  visible prompt (agent CLIs); null → anchor at the engine cursor. */
  getAnchorOverride?: () => { row: number; col: number } | null
}

const toCssColor = (rgb: number): string => `#${(rgb & 0xffffff).toString(16).padStart(6, '0')}`

export function createAtermCompositionView(deps: AtermCompositionViewDeps): AtermCompositionView {
  const { canvas, textarea, term, metrics, themeColors } = deps

  // Same stacked-2d-overlay pattern as the search overlay: the grid canvas may be
  // webgl2/OffscreenCanvas-owned, so preedit text needs its own 2d surface.
  // Created lazily on the first composition — most sessions never compose.
  let overlay: HTMLCanvasElement | null = null
  let ctx: CanvasRenderingContext2D | null = null
  let active = false

  const ensureOverlay = (): CanvasRenderingContext2D | null => {
    if (!overlay) {
      overlay = document.createElement('canvas')
      overlay.dataset.testid = 'aterm-composition-overlay'
      overlay.style.position = 'absolute'
      overlay.style.left = '0'
      overlay.style.top = '0'
      overlay.style.pointerEvents = 'none'
      overlay.style.display = 'block'
      // Above the grid canvas, below the helpers box (z-index 5).
      overlay.style.zIndex = '4'
      canvas.parentElement?.appendChild(overlay)
      ctx = overlay.getContext('2d')
    }
    return ctx
  }

  // The grid canvas's CSS box (its buffer may be worker/OffscreenCanvas-owned
  // and unreadable here) → device px via the live dpr.
  const gridDeviceSize = (): { width: number; height: number } => {
    const dpr = metrics.dpr || 1
    return {
      width: Math.max(1, Math.round(canvas.clientWidth * dpr)),
      height: Math.max(1, Math.round(canvas.clientHeight * dpr))
    }
  }

  // Cursor cell origin in DEVICE px. Worker path: snapshot cursor (≤1-frame lag,
  // the documented worker-input tradeoff); clamped so a scrolled-back viewport
  // can't push the anchor outside the pane. An anchor override (agent CLIs that
  // draw their prompt while parking the real cursor on a blank row) wins.
  const cursorDeviceOrigin = (): { x: number; y: number } => {
    const grid = gridDeviceSize()
    const cols = Math.max(1, Math.floor(grid.width / Math.max(1, metrics.cellWidth)))
    const rows = Math.max(1, Math.floor(grid.height / Math.max(1, metrics.cellHeight)))
    const anchor = deps.getAnchorOverride?.() ?? null
    const col = Math.min(Math.max(anchor?.col ?? term.cursor_x, 0), cols - 1)
    const row = Math.min(Math.max(anchor?.row ?? term.cursor_y, 0), rows - 1)
    return { x: col * metrics.cellWidth, y: row * metrics.cellHeight }
  }

  const anchorTextareaToCursor = (): void => {
    const dpr = metrics.dpr || 1
    const origin = cursorDeviceOrigin()
    // The helpers box sits at the screen origin, so CSS offsets are pane-local.
    textarea.style.left = `${origin.x / dpr}px`
    textarea.style.top = `${origin.y / dpr}px`
    // Give the (still invisible) textarea the cell's footprint so IMEs that size
    // the candidate window from the field don't collapse it to a point.
    textarea.style.width = `${metrics.cellWidth / dpr}px`
    textarea.style.height = `${metrics.cellHeight / dpr}px`
  }

  const parkTextarea = (): void => {
    Object.assign(textarea.style, ATERM_TEXTAREA_PARKED_STYLE)
  }

  const clearOverlay = (): void => {
    if (overlay && ctx) {
      ctx.clearRect(0, 0, overlay.width, overlay.height)
    }
  }

  const paintPreedit = (preedit: string): void => {
    const context = ensureOverlay()
    if (!context || !overlay) {
      return
    }
    // Mirror the grid canvas's CSS box (device px via dpr) each paint so device
    // coords align 1:1 across resizes / DPI moves.
    const grid = gridDeviceSize()
    if (overlay.width !== grid.width || overlay.height !== grid.height) {
      overlay.width = grid.width
      overlay.height = grid.height
    }
    const dpr = metrics.dpr || 1
    overlay.style.width = `${grid.width / dpr}px`
    overlay.style.height = `${grid.height / dpr}px`
    context.clearRect(0, 0, overlay.width, overlay.height)
    if (!preedit) {
      return
    }
    const cellHeight = metrics.cellHeight
    const origin = cursorDeviceOrigin()
    context.font = `${Math.max(1, Math.round(cellHeight * 0.75))}px monospace`
    context.textBaseline = 'middle'
    const textWidth = Math.ceil(context.measureText(preedit).width)
    // Keep the preedit on-screen when the cursor is near the right edge.
    const x = Math.max(0, Math.min(origin.x, overlay.width - textWidth))
    context.fillStyle = toCssColor(themeColors.bg)
    context.fillRect(x, origin.y, textWidth, cellHeight)
    context.fillStyle = toCssColor(themeColors.fg)
    context.fillText(preedit, x, origin.y + cellHeight / 2)
    // Underline marks the run as uncommitted (standard preedit affordance).
    const underline = Math.max(1, Math.round(dpr))
    context.fillRect(x, origin.y + cellHeight - underline, textWidth, underline)
  }

  return {
    begin: () => {
      active = true
      anchorTextareaToCursor()
    },
    update: (preedit) => {
      if (!active) {
        return
      }
      anchorTextareaToCursor()
      paintPreedit(preedit)
    },
    end: () => {
      active = false
      clearOverlay()
      parkTextarea()
    },
    dispose: () => {
      active = false
      overlay?.remove()
      overlay = null
      ctx = null
      parkTextarea()
    }
  }
}
