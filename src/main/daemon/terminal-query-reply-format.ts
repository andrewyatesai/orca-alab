/**
 * Pure reply formatters for the daemon query responder
 * (docs/reference/terminal-query-authority.md). Byte-for-byte parity with the
 * renderer engine's own answers so a parked/hidden pane replies identically to
 * a visible one. The engine (aterm, drop-in for xterm.js) advertises the
 * xterm.js device identity, so DA/XTVERSION reuse those constants.
 */
import type { TerminalViewCursorStyle } from '../../shared/terminal-view-attributes'

/** VT100 with Advanced Video Option — the default xterm.js DA1 identity. */
export const DA1_REPLY = '\x1b[?1;2c'
/** ConPTY 1.22+ blocks at spawn on DA1; the override identity must win. */
export const CONPTY_DA1_REPLY = '\x1b[?61;4c'
/** DA2: model 0, firmware 276, 0 — the xterm.js secondary identity. */
export const DA2_REPLY = '\x1b[>0;276;0c'
/** DSR operating-status: terminal OK. */
export const DSR_OK_REPLY = '\x1b[0n'
/** XTVERSION (CSI > q): the xterm.js version string aterm mirrors. */
export const XTVERSION_REPLY = '\x1bP>|xterm.js(6.0.0)\x1b\\'

/** CPR / DECXCPR from a 0-based engine cursor. Private (DECXCPR, answer to
 *  CSI ? 6 n) carries the `?`; plain CPR (CSI 6 n) does not. */
export function formatCursorPositionReport(
  row0: number,
  col0: number,
  isPrivate: boolean
): string {
  const marker = isPrivate ? '?' : ''
  return `\x1b[${marker}${row0 + 1};${col0 + 1}R`
}

/** Kitty keyboard flags report (answer to CSI ? u). */
export function formatKittyKeyboardFlagsReply(flags: number): string {
  return `\x1b[?${flags}u`
}

/** DECSCUSR shape value for a pushed cursor style + blink (1..6). */
export function decCursorStyleValue(style: TerminalViewCursorStyle, blink: boolean): number {
  const base = style === 'block' ? 1 : style === 'underline' ? 3 : 5
  // Odd = blinking, even = steady, in each block/underline/bar pair.
  return blink ? base : base + 1
}

export type DecrqssState = {
  scrollTop: number
  scrollBottom: number
  cursorStyleValue: number
}

/** DECRQSS status-string reply (DCS 1 $ r … ST for valid, DCS 0 $ r ST for
 *  unsupported). `request` is the setting selector after `$q`. */
export function formatDecrqssReply(request: string, state: DecrqssState): string {
  switch (request) {
    case 'r': // DECSTBM top/bottom margins
      return `\x1bP1$r${state.scrollTop};${state.scrollBottom}r\x1b\\`
    case ' q': // DECSCUSR cursor style
      return `\x1bP1$r${state.cursorStyleValue} q\x1b\\`
    case '"q': // DECSCA character protection (default off)
      return '\x1bP1$r0"q\x1b\\'
    case 'm': // SGR — aterm exposes no live pen; report the default attributes
      return '\x1bP1$r0m\x1b\\'
    default:
      return '\x1bP0$r\x1b\\'
  }
}
