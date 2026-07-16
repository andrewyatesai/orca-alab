import type { Terminal } from './aterm/terminal-types'
import type { ScrollState } from './pane-manager-types'

type ReflowLineReader = {
  getCellMetrics: (lineY: number, column: number) => { code: number; width: number } | undefined
  getLogicalLength: (lineY: number) => number | undefined
  isWrapped: (lineY: number) => boolean
  /** Rewind any peek scrolling back to the viewport the walk started from. */
  restoreViewport: () => void
}

export function captureLogicalLineAnchor(
  terminal: Terminal,
  viewportY: number
): { cellOffset: number; lineY: number } | undefined {
  const buf = terminal.buffer.active
  if (typeof buf.getLine !== 'function' || shouldKeepPhysicalResizeAnchor(terminal)) {
    return undefined
  }
  const lines = createReflowLineReader(terminal)
  try {
    let lineY = viewportY
    while (lineY > 0 && lines.isWrapped(lineY)) {
      lineY -= 1
    }
    const cursorLineY = buf.baseY + buf.cursorY
    // Why: keep upstream's conservative default (no logical anchor when the
    // anchor's logical line contains the cursor line) — the fork's facade has no
    // reflowCursorLine option; the physical marker handles the cursor-line case.
    if (lineContainsLine(lines, lineY, cursorLineY)) {
      return undefined
    }
    let cellOffset = 0
    for (let currentLineY = lineY; currentLineY < viewportY; currentLineY += 1) {
      cellOffset += readReflowedRowCellCount(terminal, lines, currentLineY)
    }
    return { cellOffset, lineY }
  } finally {
    lines.restoreViewport()
  }
}

function shouldKeepPhysicalResizeAnchor(terminal: Terminal): boolean {
  const windowsPty = terminal.options?.windowsPty
  if (!windowsPty?.buildNumber) {
    return false
  }
  // Why: xterm disables reflow only when an explicit legacy build is present;
  // Orca's backend-only fallback for an unknown Windows build still reflows.
  return windowsPty.backend !== 'conpty' || windowsPty.buildNumber < 21376
}

function lineContainsLine(
  lines: ReflowLineReader,
  logicalStartY: number,
  targetY: number
): boolean {
  if (targetY < logicalStartY) {
    return false
  }
  for (let lineY = logicalStartY + 1; lineY <= targetY; lineY += 1) {
    if (!lines.isWrapped(lineY)) {
      return false
    }
  }
  return true
}

export function resolveLogicalCellOffsetLine(
  terminal: Terminal,
  logicalStartY: number,
  cellOffset: number
): number {
  const buf = terminal.buffer.active
  const lines = createReflowLineReader(terminal)
  try {
    let lineY = logicalStartY
    let remainingCells = cellOffset
    while (lineY < buf.baseY && lines.isWrapped(lineY + 1)) {
      const rowCells = readReflowedRowCellCount(terminal, lines, lineY)
      if (remainingCells < rowCells) {
        break
      }
      remainingCells -= rowCells
      lineY += 1
    }
    return lineY
  } finally {
    lines.restoreViewport()
  }
}

/** The oldest retained scrollback line (absolute index), read from the live
 *  engine by clamping a scroll at line 0, or undefined when the host cannot be
 *  observed synchronously (worker mirror) — callers then skip renumber
 *  compensation instead of using a stale read. */
export function readOldestRetainedLine(terminal: Terminal): number | undefined {
  const buf = terminal.buffer.active
  if (typeof buf.getLine !== 'function') {
    return undefined
  }
  const home = buf.viewportY
  // The oldest line can never exceed the current viewport top.
  if (home <= 0) {
    return home === 0 ? 0 : undefined
  }
  // A buffer that can read the line above the viewport top has full-range
  // reads (xterm-shaped: markers track reflow natively) — no renumber
  // compensation, and no scroll dance. The aterm facade is viewport-scoped,
  // so this line is never readable in place.
  if (buf.getLine(home - 1)) {
    return undefined
  }
  try {
    terminal.scrollToLine(0)
    const seen = buf.viewportY
    if (seen === home) {
      // Ambiguous: the viewport already sat on the oldest line (a reflow clamp
      // does this), or an async (worker) host applies scrolls later. Nudge one
      // line down — a synchronous host must observably move.
      terminal.scrollToLine(home + 1)
      const moved = buf.viewportY !== home
      terminal.scrollToLine(home)
      return moved ? seen : undefined
    }
    terminal.scrollToLine(home)
    return seen
  } catch {
    try {
      terminal.scrollToLine(home)
    } catch {
      // Scroll APIs can throw during WebGL teardown; the next fit re-places
      // the viewport.
    }
    return undefined
  }
}

/** How far aterm's width-rewrap renumbering shifted the absolute lines the
 *  captured markers were anchored on. Zero unless a rewrap renumbering is
 *  certain: renumbering restarts the oldest retained line at the pre-resize
 *  baseY, so oldest >= captured baseY cannot be reached by eviction alone
 *  without the captured region being gone entirely. */
export function readMarkerRenumberDelta(terminal: Terminal, state: ScrollState): number {
  if (state.capturedOldestLine === undefined) {
    return 0
  }
  const hasLiveMarker =
    (state.firstVisibleLineMarker !== undefined && !state.firstVisibleLineMarker.isDisposed) ||
    (state.firstVisibleLogicalLineMarker !== undefined &&
      !state.firstVisibleLogicalLineMarker.isDisposed)
  if (!hasLiveMarker) {
    return 0
  }
  const oldestNow = readOldestRetainedLine(terminal)
  if (oldestNow === undefined || oldestNow < state.baseY) {
    return 0
  }
  return oldestNow - state.capturedOldestLine
}

function readReflowedRowCellCount(
  terminal: Terminal,
  lines: ReflowLineReader,
  lineY: number
): number {
  const cols = Math.max(terminal.cols, 1)
  const nextFirstCell = lines.getCellMetrics(lineY + 1, 0)
  if (nextFirstCell?.width !== 2) {
    return cols
  }
  // Why: a width-2 glyph wraps one cell early when only the last column
  // remains, and that placeholder is not part of the logical cell offset.
  // Fork: aterm reports the placeholder as a blank space cell (not xterm's
  // NUL), so detect it by the row's logical length stopping one cell short.
  return lines.getLogicalLength(lineY) === cols - 1 ? cols - 1 : cols
}

function createReflowLineReader(terminal: Terminal): ReflowLineReader {
  // Fork: the facade's buffer reads are thin views over engine DISPLAY rows, so
  // rows outside the viewport are unreadable in place. Peek them by scrolling
  // the real engine (scroll_search_line_into_view clamps safely), then rewind
  // to the starting viewport once the walk finishes.
  const buffer = terminal.buffer.active
  const homeViewportY = buffer.viewportY
  let peeked = false
  const lineAt = (lineY: number): ReturnType<typeof buffer.getLine> => {
    const line = buffer.getLine(lineY)
    if (line || lineY < 0) {
      return line
    }
    try {
      terminal.scrollToLine(lineY)
    } catch {
      return undefined
    }
    peeked = true
    return buffer.getLine(lineY)
  }
  return {
    isWrapped: (lineY) => lineAt(lineY)?.isWrapped ?? false,
    getLogicalLength: (lineY) => lineAt(lineY)?.length,
    getCellMetrics: (lineY, column) => {
      const cell = lineAt(lineY)?.getCell(column)
      return cell ? { code: cell.getCode(), width: cell.getWidth() } : undefined
    },
    restoreViewport: () => {
      if (!peeked) {
        return
      }
      try {
        terminal.scrollToLine(homeViewportY)
      } catch {
        // Scroll APIs can throw during WebGL teardown; the caller's own scroll
        // (or the next fit) re-places the viewport.
      }
    }
  }
}
