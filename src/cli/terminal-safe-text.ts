// Why: CLI text formatters interpolate attacker-controlled strings (web page <title>,
// URLs, OS/app window titles) straight into console.log output. Left raw they can carry
// ESC/CSI/OSC sequences to the TTY (cursor/output spoofing, OSC-8 link + OSC-52 clipboard
// abuse, and prompt-injection framing when an agent pipes stdout to an LLM). Removing the
// C0/C1 controls below (ESC/CR/LF and the C1 CSI/OSC introducers) neutralizes any escape
// sequence while leaving its now-inert bytes as harmless printable text.
const TAB = 0x09

function isUntrustedTerminalControlChar(codePoint: number): boolean {
  // C0 controls (0x00-0x1F) except tab, DEL (0x7F), and C1 controls (0x80-0x9F).
  if (codePoint === TAB) {
    return false
  }
  return codePoint <= 0x1f || codePoint === 0x7f || (codePoint >= 0x80 && codePoint <= 0x9f)
}

export function sanitizeUntrustedTerminalText(value: string): string {
  let result = ''
  for (const char of value) {
    const codePoint = char.codePointAt(0) ?? 0
    if (!isUntrustedTerminalControlChar(codePoint)) {
      result += char
    }
  }
  return result
}
