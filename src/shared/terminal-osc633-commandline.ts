/**
 * Chunk-boundary-safe OSC 633;E (command line) scanner (#7596).
 *
 * Orca's own shell hooks emit `ESC ] 633;E;<escaped-command> BEL` right before
 * `133;C`, so a cold-restore replay of the raw PTY log can recover the last
 * command a restored terminal ran. Mirrors the carry semantics of
 * `terminal-osc133-command-finished.ts` (split prefixes, BEL/ST terminators)
 * but retains only the LAST complete sequence instead of firing events.
 */

const OSC_633_E_PREFIX = '\x1b]633;E;'
// Why: our hooks truncate at 2 KB; anything far past that is a foreign/corrupt
// sequence — skip its bytes until the terminator instead of buffering forever.
const MAX_OSC_CARRY_LENGTH = 8192

type OscTerminator = {
  index: number
  length: number
}

function findOscTerminator(data: string, startIndex: number): OscTerminator | null {
  const bel = data.indexOf('\x07', startIndex)
  const st = data.indexOf('\x1b\\', startIndex)
  if (bel === -1 && st === -1) {
    return null
  }
  if (bel !== -1 && (st === -1 || bel < st)) {
    return { index: bel, length: 1 }
  }
  return { index: st, length: 2 }
}

function findPrefixCarry(data: string): string {
  const maxCarryLength = Math.min(data.length, OSC_633_E_PREFIX.length - 1)
  for (let length = maxCarryLength; length > 0; length -= 1) {
    const suffix = data.slice(data.length - length)
    if (OSC_633_E_PREFIX.startsWith(suffix)) {
      return suffix
    }
  }
  return ''
}

/** Undo the VS Code 633;E payload escaping: `\\` → `\`, `\x3b` → `;`, `\x0a` → newline. */
export function unescapeOsc633Commandline(payload: string): string {
  let result = ''
  for (let i = 0; i < payload.length; i += 1) {
    const char = payload[i]
    if (char !== '\\') {
      result += char
      continue
    }
    const next = payload[i + 1]
    if (next === '\\') {
      result += '\\'
      i += 1
      continue
    }
    if (next === 'x' && /^[0-9a-fA-F]{2}$/.test(payload.slice(i + 2, i + 4))) {
      result += String.fromCharCode(Number.parseInt(payload.slice(i + 2, i + 4), 16))
      i += 3
      continue
    }
    // Unknown escape: keep the backslash verbatim (a shell command may end in one).
    result += '\\'
  }
  return result
}

export type Osc633CommandlineScanner = {
  /** Feed one raw PTY chunk (order matters; terminators may span chunks). */
  scan: (data: string) => void
  /** The unescaped command of the last COMPLETE 633;E seen, or null. */
  lastCommandline: () => string | null
  /** Drop the carry and remembered command (parser reset / teardown). */
  reset: () => void
}

export function createOsc633CommandlineScanner(): Osc633CommandlineScanner {
  let carry = ''
  let last: string | null = null
  // Why: an oversized unterminated sequence is dropped, but its terminator has
  // not arrived yet — swallow bytes until it does so later sequences still parse.
  let skipUntilTerminator = false

  const scan = (data: string): void => {
    let combined = carry + data
    carry = ''

    if (skipUntilTerminator) {
      const terminator = findOscTerminator(combined, 0)
      if (!terminator) {
        return
      }
      skipUntilTerminator = false
      combined = combined.slice(terminator.index + terminator.length)
    }

    while (combined.length > 0) {
      const start = combined.indexOf(OSC_633_E_PREFIX)
      if (start === -1) {
        carry = findPrefixCarry(combined)
        return
      }

      const payloadStart = start + OSC_633_E_PREFIX.length
      const terminator = findOscTerminator(combined, payloadStart)
      if (!terminator) {
        carry = combined.slice(start)
        if (carry.length > MAX_OSC_CARRY_LENGTH) {
          carry = ''
          skipUntilTerminator = true
        }
        return
      }

      // A nonce-suffixed emission is `633;E;<command>;<nonce>` — the command's
      // own `;` are escaped as \x3b, so the first raw `;` ends the command.
      const payload = combined.slice(payloadStart, terminator.index)
      const commandField = payload.split(';', 1)[0] ?? ''
      last = unescapeOsc633Commandline(commandField)
      combined = combined.slice(terminator.index + terminator.length)
    }
  }

  return {
    scan,
    lastCommandline: () => last,
    reset() {
      carry = ''
      skipUntilTerminator = false
      last = null
    }
  }
}
