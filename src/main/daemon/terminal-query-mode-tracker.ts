/**
 * DECRQM (CSI Ps $ p / CSI ? Ps $ p) answer source for the daemon query
 * responder (docs/reference/terminal-query-authority.md). aterm's napi does
 * not expose per-mode DEC state, so the responder tracks DECSET/DECRST from
 * the same byte stream it answers on — engine-independent and honest (it
 * reports exactly what the program set), seeded with xterm's per-mode
 * defaults so an un-mutated mode still answers set/reset (never 0) parity.
 *
 * Cursor-blink (?12) is intentionally NOT here: it is a renderer view
 * attribute, answered from the pushed snapshot by the responder.
 */

// DECRQM report values: 1 = set, 2 = reset, 0 = not recognized.
const DECRQM_SET = 1
const DECRQM_RESET = 2
const DECRQM_UNRECOGNIZED = 0

// Recognized DEC private modes and their power-on default (enabled?). Matches
// xterm's defaults so a query for an untouched mode answers like a visible
// pane would.
const PRIVATE_MODE_DEFAULTS: Record<number, boolean> = {
  1: false, // DECCKM application cursor keys
  6: false, // DECOM origin mode
  7: true, // DECAWM autowrap
  9: false, // X10 mouse
  25: true, // DECTCEM cursor visible
  45: false, // reverse wraparound
  47: false, // alt buffer
  66: false, // DECNKM numeric keypad
  1000: false, // VT200 mouse
  1002: false, // button-event mouse
  1003: false, // any-event mouse
  1004: false, // focus reporting
  1005: false, // utf8 mouse
  1006: false, // SGR mouse
  1015: false, // urxvt mouse
  1016: false, // SGR-pixels mouse
  1047: false, // alt buffer
  1048: false, // save cursor
  1049: false, // alt buffer + save cursor
  2004: false, // bracketed paste
  2026: false, // synchronized output
  2031: false // color-scheme change notifications
}

const ANSI_MODE_DEFAULTS: Record<number, boolean> = {
  4: false, // IRM insert/replace
  20: false // LNM line feed / new line
}

export type TerminalQueryModeTracker = {
  /** Record a DECSET/DECRST (or ANSI SM/RM) applied to `mode`. */
  record: (isPrivate: boolean, mode: number, enabled: boolean) => void
  /** DECRQM report value for `mode` (1 set, 2 reset, 0 unrecognized). */
  resolve: (isPrivate: boolean, mode: number) => number
  /** RIS full reset — drop every tracked override back to defaults. */
  reset: () => void
}

export function createTerminalQueryModeTracker(): TerminalQueryModeTracker {
  const privateState = new Map<number, boolean>()
  const ansiState = new Map<number, boolean>()

  const record = (isPrivate: boolean, mode: number, enabled: boolean): void => {
    const defaults = isPrivate ? PRIVATE_MODE_DEFAULTS : ANSI_MODE_DEFAULTS
    if (!(mode in defaults)) {
      return
    }
    ;(isPrivate ? privateState : ansiState).set(mode, enabled)
  }

  const resolve = (isPrivate: boolean, mode: number): number => {
    const defaults = isPrivate ? PRIVATE_MODE_DEFAULTS : ANSI_MODE_DEFAULTS
    if (!(mode in defaults)) {
      return DECRQM_UNRECOGNIZED
    }
    const state = isPrivate ? privateState : ansiState
    const enabled = state.get(mode) ?? defaults[mode]
    return enabled ? DECRQM_SET : DECRQM_RESET
  }

  return {
    record,
    resolve,
    reset: () => {
      privateState.clear()
      ansiState.clear()
    }
  }
}
