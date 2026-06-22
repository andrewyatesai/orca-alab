import { loadAterm } from './load-aterm'
import { encodeKeyEventToBytes } from './aterm-key-encoding'
import { resolveAtermThemeColors } from './aterm-theme-colors'
import { attachAtermScrollInput } from './aterm-scroll-input'
import { attachAtermSelectionInput } from './aterm-selection-input'
import { attachAtermLinkInput } from './aterm-link-input'
import { buildAtermInputDom } from './aterm-input-dom'
import { openHttpLink } from '../../http-link-routing'
import { openTerminalHttpLink } from '../../../components/terminal-pane/terminal-url-link-hit-testing'
import type { AtermTerminal } from './aterm_wasm.js'

// Font cell size in CSS pixels; multiplied by devicePixelRatio for the engine.
export const ATERM_RENDERER_FONT_PX = 14

export type AtermPaneInputSink = (data: string) => void
export type AtermPaneResizeSink = (cols: number, rows: number) => void

/** Optional pane-scoped link routing context. When supplied, terminal URL
 *  clicks honor orca's in-app/system-browser preference exactly like the default
 *  xterm path; absent, links open via openHttpLink (worktree-scoped or, with no
 *  worktree, the system browser). Kept optional so the controller signature stays
 *  backward-compatible for callers that don't thread link context. */
export type AtermLinkContext = {
  worktreeId?: string | null
  requestOpenLinksInAppPreference?: (url: string) => boolean | Promise<boolean> | null | undefined
}

export type AtermPaneController = {
  /** Feed PTY/replay output bytes; coalesces draws into one rAF frame. */
  process: (data: string) => void
  /** Lines the viewport is scrolled up from the live bottom (0 = at bottom). */
  displayOffset: () => number
  /** Scroll scrollback (positive = older); redraws. Mirrors the wheel path. */
  scrollLines: (delta: number) => void
  /** Current selection text, if any (empty string when nothing is selected). */
  selectionText: () => string
  /** Detected link at a display cell (url + kind), or null — for hit-test/tests. */
  linkAt: (row: number, col: number) => { url: string; kind: number } | null
  dispose: () => void
}

const MIN_GRID_COLS = 1
const MIN_GRID_ROWS = 1
const DEFAULT_GRID_COLS = 80
const DEFAULT_GRID_ROWS = 24

function computeGrid(
  container: HTMLElement,
  dpr: number,
  cellWidth: number,
  cellHeight: number
): { cols: number; rows: number } {
  const deviceWidth = container.clientWidth * dpr
  const deviceHeight = container.clientHeight * dpr
  // Container not laid out yet (hidden/background pane, pre-mount): render a
  // standard 80x24 so the terminal is usable; the ResizeObserver corrects it
  // once the pane has real dimensions. Never render a 1x1 terminal.
  if (deviceWidth < cellWidth || deviceHeight < cellHeight) {
    return { cols: DEFAULT_GRID_COLS, rows: DEFAULT_GRID_ROWS }
  }
  const cols = Math.max(MIN_GRID_COLS, Math.floor(deviceWidth / cellWidth))
  const rows = Math.max(MIN_GRID_ROWS, Math.floor(deviceHeight / cellHeight))
  return { cols, rows }
}

export async function createAtermPaneController(
  container: HTMLElement,
  onInput: AtermPaneInputSink,
  onResize: AtermPaneResizeSink,
  linkContext?: AtermLinkContext
): Promise<AtermPaneController> {
  const canvas = document.createElement('canvas')
  canvas.dataset.testid = 'aterm-canvas' // e2e locator for the aterm-rendered pane
  // Fill the pane; pixelated keeps the CPU-rasterized framebuffer crisp when
  // the device-pixel canvas is scaled to CSS pixels.
  canvas.style.width = '100%'
  canvas.style.height = '100%'
  canvas.style.display = 'block'
  canvas.style.imageRendering = 'pixelated'
  canvas.style.outline = 'none'
  // Mirror xterm's DOM so the app's focus/paste/IME/clipboard logic (which keys
  // off .xterm-helper-textarea / closest('.xterm')) works unchanged. Keyboard
  // focus lives on the hidden helper textarea; the canvas keeps pixels +
  // selection-drag + wheel.
  const inputDom = buildAtermInputDom(canvas)
  container.appendChild(inputDom.wrapper)

  const ctx = canvas.getContext('2d')
  const { AtermTerminal: AtermTerminalCtor, fontBytes } = await loadAterm()

  const dpr = window.devicePixelRatio || 1
  // Seed the renderer's default fg/bg/cursor/selection from orca's active
  // terminal theme so the canvas matches the rest of the app at pane creation.
  const themeColors = resolveAtermThemeColors()
  // Build once at an arbitrary 1x1 grid to read the engine's cell metrics, then
  // size the real grid to the container.
  const term: AtermTerminal = new AtermTerminalCtor(
    MIN_GRID_ROWS,
    MIN_GRID_COLS,
    fontBytes,
    Math.round(ATERM_RENDERER_FONT_PX * dpr),
    themeColors.fg,
    themeColors.bg,
    themeColors.cursor,
    themeColors.selection
  )
  const cellWidth = term.cell_width
  const cellHeight = term.cell_height

  const inputSink = onInput
  const resizeSink = onResize
  let disposed = false
  let drawScheduled = false
  const initialGrid = computeGrid(container, dpr, cellWidth, cellHeight)
  term.resize(initialGrid.rows, initialGrid.cols)
  let cols = initialGrid.cols
  let rows = initialGrid.rows

  const draw = (): void => {
    if (disposed || !drawScheduled || !ctx) {
      return
    }
    drawScheduled = false
    term.render()
    const width = term.width
    const height = term.height
    canvas.width = width
    canvas.height = height
    // CSS size in logical pixels so the device-pixel framebuffer maps 1:1.
    canvas.style.width = `${width / dpr}px`
    canvas.style.height = `${height / dpr}px`
    ctx.putImageData(new ImageData(new Uint8ClampedArray(term.rgba()), width, height), 0, 0)
  }

  const scheduleDraw = (): void => {
    if (drawScheduled || disposed) {
      return
    }
    drawScheduled = true
    requestAnimationFrame(draw)
    // rAF is paused for hidden/occluded windows; a timer guarantees the draw
    // still lands (background panes, headless e2e). `draw` is idempotent.
    setTimeout(draw, 33)
  }

  const process = (data: string): void => {
    if (disposed) {
      return
    }
    // Follow the bottom on new output ONLY if already at the bottom; if the user
    // has scrolled up to read history, leave the viewport pinned (aterm's SCR-1
    // keeps it stable while scrollback grows).
    const wasAtBottom = term.display_offset === 0
    term.process(new TextEncoder().encode(data))
    if (wasAtBottom && term.display_offset !== 0) {
      term.scroll_to_bottom()
    }
    scheduleDraw()
  }

  // Copy selected text via Electron's clipboard IPC (same seam the rest of the
  // app uses); also surface it on a window field so e2e can assert copies under
  // a hidden window where navigator.clipboard is unavailable.
  const copyToClipboard = (text: string): void => {
    ;(window as unknown as { __atermLastCopied?: string }).__atermLastCopied = text
    void window.api?.ui?.writeClipboardText?.(text)?.catch(() => {
      /* ignore clipboard write failures */
    })
  }

  const selectionInput = attachAtermSelectionInput({
    canvas,
    term,
    dpr,
    cellWidth,
    cellHeight,
    redraw: scheduleDraw,
    isDisposed: () => disposed,
    onCopy: copyToClipboard
  })

  const scrollInput = attachAtermScrollInput({
    canvas,
    term,
    dpr,
    cellHeight,
    getRows: () => rows,
    redraw: scheduleDraw,
    isDisposed: () => disposed
  })

  // Open a clicked URL via orca's opener so the in-app/system-browser preference
  // (and Shift→system-browser escape hatch) is honored just like the xterm path.
  const openUrl = (url: string, opts: { forceSystemBrowser: boolean }): void => {
    if (linkContext?.requestOpenLinksInAppPreference) {
      openTerminalHttpLink(url, {
        worktreeId: linkContext.worktreeId ?? '',
        forceSystemBrowser: opts.forceSystemBrowser,
        requestOpenLinksInAppPreference: linkContext.requestOpenLinksInAppPreference
      })
      return
    }
    openHttpLink(url, {
      worktreeId: linkContext?.worktreeId ?? undefined,
      forceSystemBrowser: opts.forceSystemBrowser
    })
  }

  const linkInput = attachAtermLinkInput({
    canvas,
    term,
    dpr,
    cellWidth,
    cellHeight,
    redraw: scheduleDraw,
    isDisposed: () => disposed,
    openUrl
  })

  const resizeObserver = new ResizeObserver(() => {
    if (disposed) {
      return
    }
    const next = computeGrid(container, dpr, cellWidth, cellHeight)
    if (next.cols === cols && next.rows === rows) {
      return
    }
    cols = next.cols
    rows = next.rows
    term.resize(rows, cols)
    // Mirror the new grid to the PTY so the child re-wraps for the new size.
    resizeSink(cols, rows)
    scheduleDraw()
  })
  resizeObserver.observe(container)

  const { textarea } = inputDom
  // Platform-correct copy modifier: Cmd on macOS, Ctrl elsewhere.
  const isMac = typeof navigator !== 'undefined' && navigator.userAgent.includes('Mac')
  let composing = false
  const onKeyDown = (event: KeyboardEvent): void => {
    // Let the IME own keys while a composition is active.
    if (event.isComposing || composing) {
      return
    }
    // Cmd/Ctrl+C copies the canvas selection (when any) instead of sending ^C,
    // matching the default terminal's copy behavior.
    const copyChord = (isMac ? event.metaKey : event.ctrlKey) && event.key.toLowerCase() === 'c'
    if (copyChord && selectionInput.copySelection()) {
      event.preventDefault()
      return
    }
    const bytes = encodeKeyEventToBytes(event)
    if (bytes === null) {
      return
    }
    event.preventDefault()
    inputSink(bytes)
    // Clear so the sink-bound textarea never accumulates the typed characters.
    textarea.value = ''
  }
  // IME: buffer composing keystrokes, then send the committed string on end.
  const onCompositionStart = (): void => {
    composing = true
  }
  const onCompositionEnd = (event: CompositionEvent): void => {
    composing = false
    if (event.data) {
      inputSink(event.data)
    }
    textarea.value = ''
  }
  textarea.addEventListener('keydown', onKeyDown)
  textarea.addEventListener('compositionstart', onCompositionStart)
  textarea.addEventListener('compositionend', onCompositionEnd)
  // No competing paste handler: the textarea's 'xterm-helper-textarea' class lets
  // the app's existing paste path own Cmd/Ctrl+V and primary-selection paste.

  // Click anywhere on the canvas starts selection (canvas mousedown) AND lands
  // keyboard focus on the helper textarea so typing routes to the PTY.
  const onPointerDown = (): void => {
    textarea.focus()
  }
  canvas.addEventListener('pointerdown', onPointerDown)

  // Report the initial grid so the PTY spawns/resizes to match the canvas.
  resizeSink(cols, rows)
  scheduleDraw()

  return {
    process,
    displayOffset: () => term.display_offset,
    scrollLines: (delta: number) => {
      if (disposed) {
        return
      }
      term.scroll_lines(delta)
      scheduleDraw()
    },
    selectionText: () => term.selection_text() ?? '',
    linkAt: (row: number, col: number) => {
      const hit = term.link_at(row, col)
      return hit ? { url: hit.url, kind: hit.kind } : null
    },
    dispose: () => {
      if (disposed) {
        return
      }
      disposed = true
      resizeObserver.disconnect()
      textarea.removeEventListener('keydown', onKeyDown)
      textarea.removeEventListener('compositionstart', onCompositionStart)
      textarea.removeEventListener('compositionend', onCompositionEnd)
      canvas.removeEventListener('pointerdown', onPointerDown)
      selectionInput.dispose()
      scrollInput.dispose()
      linkInput.dispose()
      inputDom.wrapper.remove()
      try {
        term.free()
      } catch {
        /* ignore */
      }
    }
  }
}
