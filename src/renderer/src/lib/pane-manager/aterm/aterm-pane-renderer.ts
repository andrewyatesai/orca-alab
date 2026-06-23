import { loadAterm } from './load-aterm'
import { attachAtermTextareaInput } from './aterm-textarea-input'
import { resolveAtermThemeColors } from './aterm-theme-colors'
import { attachAtermScrollInput } from './aterm-scroll-input'
import { attachAtermSelectionInput } from './aterm-selection-input'
import { attachAtermLinkInput, type AtermFileLinkOpener } from './aterm-link-input'
import {
  createAtermSearchController,
  type AtermSearchController,
  type AtermSearchMatch
} from './aterm-search'
import { paintAtermSearchHighlights } from './aterm-search-overlay'
import { buildAtermInputDom } from './aterm-input-dom'
import { createAtermUrlOpener, type AtermLinkContext } from './aterm-url-link-routing'
import { e2eConfig } from '@/lib/e2e-config'
import type { AtermTerminal } from './aterm_wasm.js'

export type { AtermLinkContext } from './aterm-url-link-routing'

// Font cell size in CSS pixels; multiplied by devicePixelRatio for the engine.
export const ATERM_RENDERER_FONT_PX = 14

export type AtermPaneInputSink = (data: string) => void
export type AtermPaneResizeSink = (cols: number, rows: number) => void

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
  /** Run an in-terminal search: highlights matches, scrolls to the nearest, and
   *  returns the match count. Empty query clears highlights. */
  findMatches: (query: string, caseSensitive: boolean) => number
  /** Move the active highlight to the next match (wraps); scrolls into view. */
  findNextMatch: () => void
  /** Move the active highlight to the previous match (wraps); scrolls into view. */
  findPreviousMatch: () => void
  /** Drop all search highlights (close / empty query). */
  clearSearch: () => void
  /** Total matches for the current query (0 when none / no query). */
  searchMatchCount: () => number
  /** 1-based active match index, or 0 when there are no matches. */
  searchActiveMatchIndex: () => number
  /** Late-bind the file-path link opener (kind 2). The lifecycle layer supplies a
   *  closure that resolves the raw path against the pane's cwd/runtime and opens
   *  it; until set, kind-2 clicks are a no-op (cursor still shows pointer). */
  setFileLinkOpener: (fn: AtermFileLinkOpener) => void
  /** Late-bind the URL link context (worktreeId + in-app-link preference) so URL
   *  clicks honor orca's open-links-in-app preference once the lifecycle has it. */
  setUrlLinkContext: (context: AtermLinkContext) => void
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
  // E2E only: stamp the exact RGB the renderer seeds as the default bg onto THIS
  // canvas (per-pane, so multiple panes don't clobber a shared global) so the
  // theme test can assert the painted top-left pixel MATCHES the configured theme
  // background (not merely "is dark"). 0x00RRGGBB → "r,g,b".
  if (e2eConfig.exposeStore) {
    const { bg } = themeColors
    canvas.dataset.atermBg = `${(bg >> 16) & 0xff},${(bg >> 8) & 0xff},${bg & 0xff}`
  }
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

  // Search-highlight state in ABSOLUTE-row coords; converted to display rows at
  // paint time so highlights track the viewport as it scrolls.
  let searchMatches: AtermSearchMatch[] = []
  let searchActiveIndex = -1

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
    // Overlay search highlights last so they sit above the rendered glyphs.
    paintAtermSearchHighlights(ctx, searchMatches, searchActiveIndex, {
      term,
      cellWidth,
      cellHeight,
      rows
    })
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
    // New output changed buffer content but search matches are stored as absolute
    // rows; re-run the active query so highlights track current content instead
    // of stranding on stale cells (the wasm search is indexed/cheap).
    if (searchController.hasActiveQuery()) {
      searchController.refresh()
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

  // Late-bound URL link context: starts from the (optional) constructor arg and
  // can be replaced by setUrlLinkContext once the React lifecycle has the pane's
  // worktreeId + in-app-link preference (the controller is created before that
  // context exists, so threading it at construction isn't always possible).
  let activeLinkContext: AtermLinkContext | undefined = linkContext

  // Open a clicked URL via orca's opener so the in-app/system-browser preference
  // (and Shift→system-browser escape hatch) is honored just like the xterm path.
  const openUrl = createAtermUrlOpener(() => activeLinkContext)

  // Late-bound file-path opener (kind 2): the lifecycle layer supplies a closure
  // with the pane's cwd/runtime context after creation. Held in a getter so the
  // link input always sees the latest binding; null until set → kind-2 no-op.
  let fileLinkOpener: AtermFileLinkOpener | null = null

  const linkInput = attachAtermLinkInput({
    canvas,
    term,
    dpr,
    cellWidth,
    cellHeight,
    redraw: scheduleDraw,
    isDisposed: () => disposed,
    openUrl,
    getFileLinkOpener: () => fileLinkOpener
  })

  // In-terminal search: the controller owns find/next/prev state and drives the
  // highlight overlay + scroll-to-match through these renderer hooks.
  const searchController: AtermSearchController = createAtermSearchController(term, {
    setSearchHighlights: (next, activeIndex) => {
      searchMatches = next
      searchActiveIndex = activeIndex
    },
    scrollToMatch: (match) => {
      if (disposed) {
        return
      }
      term.scroll_search_line_into_view(match.line)
    },
    redraw: scheduleDraw
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
  // Keyboard + text wiring (keydown = non-text keys, 'input'/IME = text/paste),
  // owned by a focused module so this controller stays under the line budget.
  // Cmd/Ctrl+V and primary-selection paste route through the app's existing paste
  // path (keyed off the 'xterm-helper-textarea' class), which setRangeText()s the
  // text and dispatches an InputEvent — captured by the input handler.
  const textareaInput = attachAtermTextareaInput({
    textarea,
    term,
    inputSink,
    copySelection: () => selectionInput.copySelection()
  })

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
    findMatches: (query: string, caseSensitive: boolean) => {
      if (disposed) {
        return 0
      }
      return searchController.find(query, caseSensitive)
    },
    findNextMatch: () => searchController.next(),
    findPreviousMatch: () => searchController.prev(),
    clearSearch: () => searchController.clear(),
    searchMatchCount: () => searchController.count(),
    searchActiveMatchIndex: () => searchController.activeIndex(),
    setFileLinkOpener: (fn: AtermFileLinkOpener) => {
      fileLinkOpener = fn
    },
    setUrlLinkContext: (context: AtermLinkContext) => {
      activeLinkContext = context
    },
    dispose: () => {
      if (disposed) {
        return
      }
      disposed = true
      resizeObserver.disconnect()
      textareaInput.dispose()
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
