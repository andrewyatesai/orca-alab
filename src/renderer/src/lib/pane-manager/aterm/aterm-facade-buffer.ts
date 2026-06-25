import type { IBufferLine, IBufferCell, IMarker } from '@xterm/xterm'
import type { AtermPaneController } from './aterm-pane-controller-types'

/** The subset of the controller the buffer facade reads. Narrowed so the buffer
 *  module doesn't depend on the full controller surface. */
export type AtermBufferSource = Pick<
  AtermPaneController,
  | 'gridSize'
  | 'isAltScreen'
  | 'baseY'
  | 'displayOriginAbsolute'
  | 'cursorX'
  | 'cursorY'
  | 'rowIsWrapped'
  | 'rowLen'
  | 'rowText'
  | 'cellText'
  | 'cellIsWide'
>

/** A getter for the live controller, or null before the async attach completes.
 *  Before the engine exists there is genuinely no buffer, so reads return
 *  accurate empties (not fakes). */
type ControllerGetter = () => AtermBufferSource | null

/** Build the IBufferCell for a display cell from the engine's grapheme + width.
 *  Only getChars()/getWidth() are consumed by orca's link translation; the SGR
 *  attribute accessors are not used there, so they return xterm's defaults. */
function buildBufferCell(
  controller: AtermBufferSource,
  displayRow: number,
  col: number
): IBufferCell {
  const chars = controller.cellText(displayRow, col)
  const wide = controller.cellIsWide(displayRow, col)
  // Width: 2 for a wide lead cell, 0 for its trailing spacer (empty grapheme,
  // not wide-flagged), 1 otherwise — matches xterm's 0/1/2 cell-width contract.
  const width = wide === true ? 2 : chars === '' ? 0 : 1
  return {
    getChars: () => chars,
    getWidth: () => width,
    // The link/selection consumers only read getChars()/getWidth(); the rest of
    // IBufferCell is required by the type but unused by orca, so return neutral
    // defaults rather than inventing attribute state the facade can't source.
    getCode: () => (chars ? (chars.codePointAt(0) ?? 0) : 0),
    isBold: () => 0,
    isItalic: () => 0,
    isDim: () => 0,
    isUnderline: () => 0,
    isBlink: () => 0,
    isInverse: () => 0,
    isInvisible: () => 0,
    isStrikethrough: () => 0,
    isOverline: () => 0,
    isAttributeDefault: () => true,
    getFgColorMode: () => 0,
    getBgColorMode: () => 0,
    getFgColor: () => -1,
    getBgColor: () => -1,
    isFgRGB: () => false,
    isBgRGB: () => false,
    isFgPalette: () => false,
    isBgPalette: () => false,
    isFgDefault: () => true,
    isBgDefault: () => true,
    getUnderlineColorMode: () => 0,
    getUnderlineColor: () => -1,
    isUnderlineColorRGB: () => false,
    isUnderlineColorPalette: () => false,
    isUnderlineColorDefault: () => true,
    getUnderlineStyle: () => 0,
    // orca's link translation never compares cell attributes; default-equal is the
    // honest answer for the neutral attributes the facade exposes.
    attributesEquals: () => true
  }
}

/** An IBufferLine-shaped line whose translateToString carries the optional
 *  `outColumns` 4th arg orca's wrapped-link translation casts to (xterm's own
 *  type omits it; the addon adds it). Omit + redeclare to avoid intersecting
 *  the 3-arg and 4-arg overloads. */
type FacadeBufferLine = Omit<IBufferLine, 'translateToString'> & {
  translateToString(
    trimRight?: boolean,
    startColumn?: number,
    endColumn?: number,
    outColumns?: number[]
  ): string
}

/** Build an IBufferLine over a single engine DISPLAY row. The display row is
 *  resolved fresh on each read so a viewport scroll between getLine() and a
 *  later read can't strand the line at a stale offset. */
function buildBufferLine(controller: AtermBufferSource, displayRow: number): FacadeBufferLine {
  const { cols } = controller.gridSize()
  return {
    isWrapped: controller.rowIsWrapped(displayRow) === true,
    // Logical length (last non-empty cell + 1); fall back to the grid width when
    // the engine can't report it (out of range), matching xterm's full-width line.
    length: controller.rowLen(displayRow) ?? cols,
    getCell: (x, _cell) => {
      if (x < 0 || x >= cols) {
        return undefined
      }
      return buildBufferCell(controller, displayRow, x)
    },
    translateToString: (trimRight, startColumn, endColumn, outColumns) => {
      const text = controller.rowText(displayRow) ?? ''
      const start = startColumn ?? 0
      const end = endColumn ?? text.length
      let sliced = text.slice(start, end)
      if (trimRight) {
        sliced = sliced.replace(/\s+$/, '')
      }
      // outColumns maps each output char index → its source column. row_text is a
      // 1:1 char→column stream for orca's link use (no wide-cell column tracking
      // is consumed), so emit identity columns offset by `start`.
      if (outColumns) {
        outColumns.length = 0
        for (let i = 0; i <= sliced.length; i++) {
          outColumns.push(start + i)
        }
      }
      return sliced
    }
  }
}

/** Convert an ABSOLUTE buffer line index (xterm semantics: 0..baseY+rows) into a
 *  live engine DISPLAY row, or null when the line isn't currently on screen. */
function absoluteToDisplayRow(controller: AtermBufferSource, absY: number): number | null {
  const { rows } = controller.gridSize()
  const displayRow = absY - controller.displayOriginAbsolute()
  return displayRow >= 0 && displayRow < rows ? displayRow : null
}

/** The xterm-compatible `buffer` object backed by live aterm engine state. */
export type AtermFacadeBuffer = {
  active: {
    readonly type: 'normal' | 'alternate'
    readonly viewportY: number
    readonly baseY: number
    readonly cursorX: number
    readonly cursorY: number
    readonly length: number
    getLine(absY: number): IBufferLine | undefined
  }
  onBufferChange(handler: (buffer: AtermFacadeBuffer['active']) => void): { dispose: () => void }
}

/** Build the facade `buffer` over a (possibly not-yet-attached) controller. All
 *  reads are live engine state; before attach they return accurate empties. */
export function createAtermFacadeBuffer(getController: ControllerGetter): {
  buffer: AtermFacadeBuffer
  registerMarker: (offset: number) => IMarker | undefined
  /** Poll the alt-screen flag and fire onBufferChange when it flips (no engine
   *  event exists, so the facade compares after each process()). */
  pollBufferChange: () => void
} {
  const bufferChangeListeners = new Set<(buffer: AtermFacadeBuffer['active']) => void>()
  let lastBufferType: 'normal' | 'alternate' | null = null

  const active: AtermFacadeBuffer['active'] = {
    get type() {
      return getController()?.isAltScreen() ? 'alternate' : 'normal'
    },
    // xterm's viewportY is the ABSOLUTE row of the top visible line (ydisp), so
    // map it to display_origin_absolute (NOT display_offset): link hit-testing
    // does `displayRow + viewportY` to recover an absolute line, and the at-bottom
    // check `viewportY >= baseY` holds because at-bottom origin == base_y.
    get viewportY() {
      return getController()?.displayOriginAbsolute() ?? 0
    },
    get baseY() {
      return getController()?.baseY() ?? 0
    },
    get cursorX() {
      return getController()?.cursorX() ?? 0
    },
    get cursorY() {
      return getController()?.cursorY() ?? 0
    },
    get length() {
      const controller = getController()
      if (!controller) {
        return 0
      }
      return controller.baseY() + controller.gridSize().rows
    },
    getLine(absY: number) {
      const controller = getController()
      if (!controller) {
        return undefined
      }
      const displayRow = absoluteToDisplayRow(controller, absY)
      // Off-screen scrollback isn't retrievable via the display-row API, so
      // return undefined (accurate) rather than a fabricated blank line.
      return displayRow === null ? undefined : buildBufferLine(controller, displayRow)
    }
  }

  const buffer: AtermFacadeBuffer = {
    active,
    onBufferChange(handler) {
      bufferChangeListeners.add(handler)
      return { dispose: () => void bufferChangeListeners.delete(handler) }
    }
  }

  const pollBufferChange = (): void => {
    const controller = getController()
    if (!controller) {
      return
    }
    const next = controller.isAltScreen() ? 'alternate' : 'normal'
    if (next !== lastBufferType) {
      lastBufferType = next
      bufferChangeListeners.forEach((listener) => listener(active))
    }
  }

  // A persistent scroll anchor over an ABSOLUTE buffer row. xterm's marker tracks
  // the same line through reflow; aterm has no marker API, so anchor the absolute
  // row at registration and map it back to a display-relative `line` on read.
  const registerMarker = (offset: number): IMarker | undefined => {
    const controller = getController()
    if (!controller) {
      return undefined
    }
    // offset is relative to the cursor (xterm semantics). Absolute anchor row =
    // (baseY + cursorY) + offset.
    const anchorAbsolute = controller.baseY() + controller.cursorY() + offset
    let disposed = false
    return {
      id: anchorAbsolute,
      get isDisposed() {
        return disposed
      },
      get line() {
        // xterm's marker.line is an ABSOLUTE buffer line; restoreScrollState
        // compares it against buf.baseY and feeds scrollToLine (also absolute).
        return anchorAbsolute
      },
      dispose() {
        disposed = true
      },
      onDispose() {
        return { dispose: () => undefined }
      }
    } as unknown as IMarker
  }

  return { buffer, registerMarker, pollBufferChange }
}
