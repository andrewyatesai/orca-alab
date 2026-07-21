/**
 * Daemon-side terminal query authority (docs/reference/terminal-query-authority.md).
 *
 * The renderer PUSHES its view attributes (theme colors, cursor style/blink);
 * this responder SCANS the PTY output stream for terminal queries and ANSWERS
 * them by writing the reply back to the PTY. It exists so parked / hidden /
 * SSH / cold panes with no live renderer attached still answer DA/DSR/CPR/
 * DECRQM/DECRQSS/XTVERSION/kitty/OSC-color probes — the aterm engine owns the
 * grid but its napi emits no replies, so the reply grammar lives here in TS.
 *
 * Reply emission is gated per chunk by `forwardQueryReplies`: seeds, hydration
 * snapshots, and delivered chunks parse with the gate closed, so a query
 * embedded in replayed bytes mutates tracked state (OSC SET overrides, mode
 * flags) but answers no one (the main-side replay guard).
 */
import { scanTerminalQuerySequences, type TerminalQueryToken } from './terminal-query-scan'
import {
  createTerminalQueryModeTracker,
  type TerminalQueryModeTracker
} from './terminal-query-mode-tracker'
import {
  CONPTY_DA1_REPLY,
  DA1_REPLY,
  DA2_REPLY,
  DSR_OK_REPLY,
  XTVERSION_REPLY,
  decCursorStyleValue,
  formatCursorPositionReport,
  formatDecrqssReply,
  formatKittyKeyboardFlagsReply
} from './terminal-query-reply-format'
import {
  installTerminalViewAttributeResponder,
  type TerminalViewAttributeResponder
} from './terminal-view-attribute-responder'
import type { TerminalViewAttributes } from '../../shared/terminal-view-attributes'

export type TerminalModelQueryResponderDeps = {
  /** Raw reply sink (already PTY-identity checked by the caller). */
  emitReply: (reply: string) => void
  /** 0-based [row, col] engine cursor for CPR / DECXCPR. */
  getCursor: () => [number, number]
  /** Live viewport rows for the default DECSTBM bottom margin. */
  getRows: () => number
}

export type TerminalModelQueryResponder = {
  ingest: (data: string, forwardQueryReplies: boolean) => void
  setViewAttributesGetter: (getter: () => TerminalViewAttributes | null) => void
  applyPushedViewAttributes: (attributes: TerminalViewAttributes) => void
  enableConptyDa1Override: () => void
  enableConptyOscColorReplySuppression: () => void
  onResize: () => void
}

// ConPTY swallows the ESC of an OSC reply written to conin and echoes the
// printable remainder (`]11;rgb:...`) into the visible prompt (#6975).
const CONPTY_LEAKY_OSC_COLOR_REPLY = /^\x1b\](?:10|11|12);/

export function createTerminalModelQueryResponder(
  deps: TerminalModelQueryResponderDeps
): TerminalModelQueryResponder {
  let pending = ''
  let forwardingActive = false
  let conptyDa1Override = false
  let conptyOscColorReplySuppression = false
  let viewAttributesGetter: () => TerminalViewAttributes | null = () => null
  const modes: TerminalQueryModeTracker = createTerminalQueryModeTracker()
  let kittyFlags = 0
  const kittyStack: number[] = []
  // null = default region (full viewport); set on DECSTBM, reset on resize/RIS.
  let scrollMargins: { top: number; bottom: number } | null = null

  const emit = (reply: string): void => {
    if (forwardingActive) {
      deps.emitReply(reply)
    }
  }

  const colorResponder: TerminalViewAttributeResponder = installTerminalViewAttributeResponder({
    getBaseAttributes: () => viewAttributesGetter(),
    // Why the filter: OSC 10/11/12 reports leak as prompt text on ConPTY; SET
    // tracking and CSI-shaped reports (?996n) are unaffected.
    emitReply: (reply) => {
      if (conptyOscColorReplySuppression && CONPTY_LEAKY_OSC_COLOR_REPLY.test(reply)) {
        return
      }
      emit(reply)
    }
  })

  const answerDeviceAttributes = (token: Extract<TerminalQueryToken, { kind: 'csi' }>): void => {
    if (token.prefix === '>') {
      emit(DA2_REPLY)
    } else if (token.prefix === '' && (token.params === '' || token.params === '0')) {
      emit(conptyDa1Override ? CONPTY_DA1_REPLY : DA1_REPLY)
    }
  }

  const answerDsr = (token: Extract<TerminalQueryToken, { kind: 'csi' }>): void => {
    const isPrivate = token.prefix === '?'
    if (!isPrivate && token.params === '5') {
      emit(DSR_OK_REPLY)
    } else if (token.params === '6') {
      const [row, col] = deps.getCursor()
      emit(formatCursorPositionReport(row, col, isPrivate))
    } else if (isPrivate && token.params === '996') {
      colorResponder.handleColorSchemeQuery()
    }
    // ?15n/?25n/?26n/?53n and everything else: silent (unsupported status).
  }

  const answerDecrqm = (token: Extract<TerminalQueryToken, { kind: 'csi' }>): void => {
    const isPrivate = token.prefix === '?'
    const mode = Number.parseInt(token.params, 10)
    if (!Number.isInteger(mode)) {
      return
    }
    const marker = isPrivate ? '?' : ''
    if (isPrivate && mode === 12) {
      // Cursor blink is a renderer view attribute, not a stream mode.
      const attrs = viewAttributesGetter()
      const value = attrs ? (attrs.cursorBlink ? 1 : 2) : 2
      emit(`\x1b[?12;${value}$y`)
      return
    }
    emit(`\x1b[${marker}${mode};${modes.resolve(isPrivate, mode)}$y`)
  }

  const trackKitty = (token: Extract<TerminalQueryToken, { kind: 'csi' }>): void => {
    if (token.prefix === '?' && token.params === '') {
      emit(formatKittyKeyboardFlagsReply(kittyFlags))
      return
    }
    if (token.prefix === '=') {
      const [flagsText, modeText] = token.params.split(';')
      const flags = Number.parseInt(flagsText || '0', 10) || 0
      const setMode = modeText ? Number.parseInt(modeText, 10) : 1
      kittyFlags = setMode === 2 ? kittyFlags | flags : setMode === 3 ? kittyFlags & ~flags : flags
    } else if (token.prefix === '>') {
      kittyStack.push(kittyFlags)
      kittyFlags = Number.parseInt(token.params || '0', 10) || 0
    } else if (token.prefix === '<') {
      const count = token.params ? Number.parseInt(token.params, 10) || 1 : 1
      for (let i = 0; i < count; i += 1) {
        kittyFlags = kittyStack.pop() ?? 0
      }
    }
  }

  const trackModeChange = (token: Extract<TerminalQueryToken, { kind: 'csi' }>): void => {
    if (token.prefix !== '' && token.prefix !== '?') {
      return
    }
    const isPrivate = token.prefix === '?'
    const enabled = token.final === 'h'
    for (const raw of token.params.split(';')) {
      if (raw === '') {
        continue
      }
      const mode = Number.parseInt(raw, 10)
      if (Number.isInteger(mode)) {
        modes.record(isPrivate, mode, enabled)
      }
    }
  }

  const trackScrollMargins = (token: Extract<TerminalQueryToken, { kind: 'csi' }>): void => {
    if (token.prefix !== '') {
      return
    }
    if (token.params === '') {
      scrollMargins = null
      return
    }
    const [top, bottom] = token.params.split(';').map((n) => Number.parseInt(n, 10))
    if (Number.isInteger(top) && Number.isInteger(bottom)) {
      scrollMargins = { top, bottom }
    }
  }

  const handleCsi = (token: Extract<TerminalQueryToken, { kind: 'csi' }>): void => {
    switch (token.final) {
      case 'c':
        answerDeviceAttributes(token)
        break
      case 'q':
        if (token.prefix === '>') {
          emit(XTVERSION_REPLY)
        }
        break
      case 'n':
        answerDsr(token)
        break
      case 'p':
        if (token.intermediates.includes('$')) {
          answerDecrqm(token)
        }
        break
      case 'u':
        trackKitty(token)
        break
      case 'h':
      case 'l':
        trackModeChange(token)
        break
      case 'r':
        trackScrollMargins(token)
        break
      default:
        break
    }
  }

  const handleDcs = (body: string): void => {
    const request = body.replace(/^\d+/, '')
    if (request.startsWith('$q')) {
      const attrs = viewAttributesGetter()
      emit(
        formatDecrqssReply(request.slice(2), {
          scrollTop: scrollMargins?.top ?? 1,
          scrollBottom: scrollMargins?.bottom ?? deps.getRows(),
          cursorStyleValue: attrs ? decCursorStyleValue(attrs.cursorStyle, attrs.cursorBlink) : 2
        })
      )
    }
    // `+q` XTGETTCAP and non-DECRQSS DCS (sixel, …): silent.
  }

  const visit = (token: TerminalQueryToken): void => {
    switch (token.kind) {
      case 'csi':
        handleCsi(token)
        break
      case 'osc':
        if (Number.isInteger(token.id)) {
          colorResponder.handleOsc(token.id, token.body)
        }
        break
      case 'dcs':
        handleDcs(token.body)
        break
      case 'ris':
        modes.reset()
        kittyFlags = 0
        kittyStack.length = 0
        scrollMargins = null
        colorResponder.clearColorOverrides()
        break
    }
  }

  return {
    ingest: (data, forwardQueryReplies) => {
      forwardingActive = forwardQueryReplies
      const result = scanTerminalQuerySequences(pending + data, visit)
      pending = result.pending
      forwardingActive = false
    },
    setViewAttributesGetter: (getter) => {
      viewAttributesGetter = getter
    },
    applyPushedViewAttributes: () => {
      // A theme apply overwrites OSC-SET-mutated colors on visible panes too
      // (ThemeService._setTheme parity); cursor style/blink are read live from
      // the getter, so nothing else is cached here.
      colorResponder.clearColorOverrides()
    },
    enableConptyDa1Override: () => {
      conptyDa1Override = true
    },
    enableConptyOscColorReplySuppression: () => {
      conptyOscColorReplySuppression = true
    },
    onResize: () => {
      scrollMargins = null
    }
  }
}
