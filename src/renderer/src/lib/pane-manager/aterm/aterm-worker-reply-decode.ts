// Byte→string decode rules for the engine reports the render worker forwards to
// the main thread (reply channel → PTY). Extracted from aterm-worker-terminal to
// keep that file under the line budget.

/** Latin-1 decode the engine's reply bytes (DA/DSR/CPR/colour are ASCII); drop any
 *  byte ≥ 0x80 rather than let the UTF-8 PTY write corrupt it (parity with the
 *  main-thread aterm-reply-drain). */
export function decodeReply(bytes: Uint8Array | undefined): string {
  if (!bytes || bytes.length === 0) {
    return ''
  }
  let out = ''
  for (let i = 0; i < bytes.length; i++) {
    if (bytes[i] < 0x80) {
      out += String.fromCharCode(bytes[i])
    }
  }
  return out
}

/** Latin-1 map ALL report bytes (0..255) for mouse reports. Legacy X10/1000/1002/1003
 *  encode coords as 32+value, which exceeds 0x7F past column/row 95 — decodeReply's
 *  ASCII-only filter would silently drop those bytes and truncate the report. Each byte
 *  maps 1:1 to a char code (no UTF-8 widening), matching the in-process aterm-mouse-input
 *  send() which posts String.fromCharCode(...bytes); the reply channel forwards it
 *  verbatim to the PTY input sink. */
export function decodeMouseReport(bytes: Uint8Array | undefined): string {
  if (!bytes || bytes.length === 0) {
    return ''
  }
  let out = ''
  for (let i = 0; i < bytes.length; i++) {
    out += String.fromCharCode(bytes[i])
  }
  return out
}
