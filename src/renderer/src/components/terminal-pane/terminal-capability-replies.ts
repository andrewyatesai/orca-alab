import type { IDisposable, IParser, Terminal } from '@xterm/xterm'

export const DEFAULT_DA1_RESPONSE = '\x1b[?1;2c'
export const CONPTY_DA1_RESPONSE = '\x1b[?61;4c'
// VT100 + AVO/printer (the default identity) + Sixel (param 4). Advertised for
// panes the aterm renderer draws — aterm rasterizes Sixel (and Kitty/iTerm2)
// images, so apps that gate Sixel on the DA1 `;4` bit will actually send it.
export const DA1_RESPONSE_WITH_SIXEL = '\x1b[?1;2;4c'

type TerminalCapabilityRepliesDeps = {
  terminal: Pick<Terminal, 'cols' | 'rows' | 'element'>
  parser: Pick<IParser, 'registerCsiHandler'>
  sendInput: (data: string) => boolean | void
  isReplaying: () => boolean
  /** DA1 reply — a string, or a getter resolved at reply time so it can depend on
   *  live pane state (e.g. whether the aterm canvas, which renders Sixel, is up). */
  da1Response?: string | (() => string)
  /** When true (aterm pane), the engine drains + forwards its OWN DA1 reply, so this
   *  renderer-side responder must NOT also answer (it would double-answer the PTY).
   *  It still CONSUMES the query so the xterm shim doesn't auto-reply. */
  isAtermReplyOwned?: () => boolean
}

function isPrimaryDeviceAttributesQuery(params: (number | number[])[]): boolean {
  return params.length === 0 || (params.length === 1 && params[0] === 0)
}

function getTerminalScreenElement(
  terminal: Pick<Terminal, 'element'>
): Pick<HTMLElement, 'getBoundingClientRect'> | null {
  if (typeof terminal.element?.querySelector !== 'function') {
    return null
  }
  return terminal.element.querySelector('.xterm-screen') ?? null
}

function measureCellPixels(
  terminal: Pick<Terminal, 'cols' | 'rows' | 'element'>
): { width: number; height: number } | null {
  if (terminal.cols <= 0 || terminal.rows <= 0) {
    return null
  }
  const rect = getTerminalScreenElement(terminal)?.getBoundingClientRect()
  if (!rect || !(rect.width > 0) || !(rect.height > 0)) {
    return null
  }
  return {
    width: Math.max(1, Math.round(rect.width / terminal.cols)),
    height: Math.max(1, Math.round(rect.height / terminal.rows))
  }
}

function disposeAll(disposables: IDisposable[]): void {
  for (const disposable of disposables) {
    disposable.dispose()
  }
}

/** Device-pixel sizes the renderer knows (text-area framebuffer + cell). The
 *  aterm canvas controller is authoritative here; an unopened xterm shim has no
 *  DOM to measure. */
export type TerminalRendererPixelSize = {
  width: number
  height: number
  cellWidth: number
  cellHeight: number
}

export function createTerminalPixelSizeQueryResponder(
  terminal: Pick<Terminal, 'cols' | 'rows' | 'element'>,
  sendInput: (data: string) => boolean | void,
  // Live aterm pixel-size source. When the pane is aterm-rendered the xterm is
  // unopened (no .xterm-screen to measure), so the canvas controller owns pixel
  // size; read it live so a settings/DPI change is reflected. Returns null when
  // not aterm-rendered (then fall back to the xterm DOM measurement).
  getRendererPixelSize?: () => TerminalRendererPixelSize | null
): (data: string) => void {
  let pending = ''
  const respond = (reportsWindowPixels: boolean): void => {
    const rendererPixels = getRendererPixelSize?.() ?? null
    if (rendererPixels) {
      const width = reportsWindowPixels ? rendererPixels.width : rendererPixels.cellWidth
      const height = reportsWindowPixels ? rendererPixels.height : rendererPixels.cellHeight
      // Skip a zero-sized framebuffer (pre-first-render) so we never emit a
      // bogus "\x1b[4;0;0t"; wait for a real size.
      if (width > 0 && height > 0) {
        sendInput(`\x1b[${reportsWindowPixels ? 4 : 6};${height};${width}t`)
      }
      return
    }
    const cell = measureCellPixels(terminal)
    if (!cell) {
      return
    }
    const width = cell.width * (reportsWindowPixels ? terminal.cols : 1)
    const height = cell.height * (reportsWindowPixels ? terminal.rows : 1)
    sendInput(`\x1b[${reportsWindowPixels ? 4 : 6};${height};${width}t`)
  }
  return (data) => {
    const input = pending + data
    pending = input.endsWith('\x1b') || input.endsWith('\x1b[') ? input.slice(-2) : ''
    let offset = 0
    while (offset < input.length) {
      const queryIndex = input.indexOf('\x1b[', offset)
      if (queryIndex === -1) {
        break
      }
      const query = input.slice(queryIndex, queryIndex + 5)
      if (query === '\x1b[14t') {
        respond(true)
        offset = queryIndex + 5
        continue
      }
      if (query === '\x1b[16t') {
        respond(false)
        offset = queryIndex + 5
        continue
      }
      offset = queryIndex + 2
    }
  }
}

/** OSC 10 (foreground) / OSC 11 (background) color sources as 0x00RRGGBB. The
 *  aterm renderer owns the theme, so it answers these; the daemon and unopened
 *  xterm shim cannot. Returns null when not aterm-rendered (no reply — the
 *  legacy xterm path renders to a DOM and answers OSC colors itself). */
export type TerminalRendererThemeColors = { fg: number; bg: number }

function u32ToXtermRgb(color: number): string {
  // xterm reports OSC color components as 16-bit (each 8-bit byte doubled), e.g.
  // 0x1a2b3c -> "rgb:1a1a/2b2b/3c3c". This matches xterm/VTE's reply format.
  const r = (color >> 16) & 0xff
  const g = (color >> 8) & 0xff
  const b = color & 0xff
  const dup = (n: number): string => n.toString(16).padStart(2, '0').repeat(2)
  return `rgb:${dup(r)}/${dup(g)}/${dup(b)}`
}

// OSC 10/11 color QUERY: ESC ] 10 ; ? ST  and  ESC ] 11 ; ? ST (ST = BEL or ESC \).
const OSC_COLOR_QUERY_FG = '\x1b]10;?'
const OSC_COLOR_QUERY_BG = '\x1b]11;?'

export function createTerminalOscColorQueryResponder(
  sendInput: (data: string) => boolean | void,
  getRendererThemeColors: () => TerminalRendererThemeColors | null,
  isReplaying: () => boolean
): (data: string) => void {
  let pending = ''
  return (data) => {
    const input = pending + data
    // Only carry an UNTERMINATED trailing partial of either query (max length 6)
    // so a query split across chunks still matches once — without re-matching a
    // fully-consumed query on the next call (that would double-reply).
    const tail = input.slice(-(OSC_COLOR_QUERY_FG.length - 1))
    pending =
      OSC_COLOR_QUERY_FG.startsWith(tail) || OSC_COLOR_QUERY_BG.startsWith(tail) ? tail : ''
    if (isReplaying()) {
      // Why: replayed scrollback may contain old OSC color queries; answering
      // those into the fresh shell would leak stray "]11;rgb:..." input.
      return
    }
    const colors = getRendererThemeColors()
    if (!colors) {
      return
    }
    // Consume the matched query span(s) so the carried tail can't re-trigger.
    const consumed = input.slice(0, input.length - pending.length)
    if (consumed.includes(OSC_COLOR_QUERY_FG)) {
      sendInput(`\x1b]10;${u32ToXtermRgb(colors.fg)}\x07`)
    }
    if (consumed.includes(OSC_COLOR_QUERY_BG)) {
      sendInput(`\x1b]11;${u32ToXtermRgb(colors.bg)}\x07`)
    }
  }
}

export function installTerminalCapabilityReplyHandlers(
  deps: TerminalCapabilityRepliesDeps
): IDisposable {
  const disposables = [
    deps.parser.registerCsiHandler({ final: 'c' }, (params) => {
      if (!isPrimaryDeviceAttributesQuery(params)) {
        return false
      }
      // Why: restored scrollback may contain old DA1 queries; answering those
      // into the fresh shell recreates the stray-input leak this handler fixes.
      // Consume the query either way so the xterm shim never auto-replies; only
      // ANSWER here when aterm isn't the owner (else aterm drains its own DA1).
      if (!deps.isReplaying() && !deps.isAtermReplyOwned?.()) {
        const da1 =
          typeof deps.da1Response === 'function' ? deps.da1Response() : deps.da1Response
        deps.sendInput(da1 ?? DEFAULT_DA1_RESPONSE)
      }
      return true
    }),
    // For aterm panes the engine drains + forwards its own DA2 / DSR-CPR / DECRQM
    // replies, so CONSUME those queries here (return true) to stop the xterm shim
    // double-answering them via onData. For the xterm fallback (not aterm-owned)
    // return false so xterm answers as before. Pure queries with no state effect, so
    // consuming is safe. (DA1 is handled above; OSC colour + CSI 14t/16t pixel-size
    // are drained by aterm and skipped renderer-side in pty-connection.)
    deps.parser.registerCsiHandler({ final: 'n' }, () => deps.isAtermReplyOwned?.() ?? false),
    deps.parser.registerCsiHandler({ prefix: '?', final: 'n' }, () =>
      deps.isAtermReplyOwned?.() ?? false
    ),
    deps.parser.registerCsiHandler({ prefix: '?', intermediates: '$', final: 'p' }, () =>
      deps.isAtermReplyOwned?.() ?? false
    ),
    deps.parser.registerCsiHandler({ prefix: '>', final: 'c' }, () =>
      deps.isAtermReplyOwned?.() ?? false
    )
  ]

  return {
    dispose: () => disposeAll(disposables)
  }
}
