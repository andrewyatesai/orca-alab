/**
 * Byte-stream tokenizer for the daemon query responder
 * (docs/reference/terminal-query-authority.md). The aterm headless engine
 * exposes no VT parser, so this walks the raw output stream itself, yielding
 * one token per complete CSI / OSC / DCS / RIS sequence. Split sequences are
 * carried in `pending` and completed against the next chunk, so a query cut
 * across a PTY/SSH chunk boundary is still answered exactly once.
 */

const ESC = '\x1b'
const BEL = '\x07'
// Bounds the carried partial so a lone ESC or a never-terminated OSC cannot
// grow unboundedly across chunks (matches the OSC/private-mode scanners).
const MAX_PENDING_CHARS = 4096

export type TerminalQueryToken =
  | { kind: 'csi'; prefix: string; params: string; intermediates: string; final: string }
  | { kind: 'osc'; id: number; body: string }
  | { kind: 'dcs'; body: string }
  | { kind: 'ris' }

function isFinalByte(code: number): boolean {
  return code >= 0x40 && code <= 0x7e
}

function isParamByte(code: number): boolean {
  // 0-9 : ;  (0x30-0x3b) — DECRQM/DA parameter substring.
  return code >= 0x30 && code <= 0x3b
}

function isIntermediateByte(code: number): boolean {
  // Space through / (0x20-0x2f): the '$' of DECRQM, '"' of DECSCA, etc.
  return code >= 0x20 && code <= 0x2f
}

function isPrivatePrefix(code: number): boolean {
  // < = > ?  (0x3c-0x3f)
  return code >= 0x3c && code <= 0x3f
}

type CsiParse = { token: TerminalQueryToken; next: number } | 'incomplete'

function parseCsi(input: string, start: number): CsiParse {
  // start points at the byte after `ESC [`.
  let i = start
  let prefix = ''
  if (i < input.length && isPrivatePrefix(input.charCodeAt(i))) {
    prefix = input[i]
    i += 1
  }
  const paramsStart = i
  while (i < input.length && isParamByte(input.charCodeAt(i))) {
    i += 1
  }
  const params = input.slice(paramsStart, i)
  const intermediatesStart = i
  while (i < input.length && isIntermediateByte(input.charCodeAt(i))) {
    i += 1
  }
  const intermediates = input.slice(intermediatesStart, i)
  if (i >= input.length) {
    return 'incomplete'
  }
  if (!isFinalByte(input.charCodeAt(i))) {
    // Malformed CSI (control char before a final byte). Give up on this
    // sequence; resume scanning after the introducer.
    return { token: { kind: 'csi', prefix, params, intermediates, final: '' }, next: i }
  }
  return {
    token: { kind: 'csi', prefix, params, intermediates, final: input[i] },
    next: i + 1
  }
}

function oscTerminatorAt(input: string, index: number): { end: number; contentEnd: number } | null {
  if (input[index] === BEL) {
    return { end: index + 1, contentEnd: index }
  }
  if (input[index] === ESC && input[index + 1] === '\\') {
    return { end: index + 2, contentEnd: index }
  }
  return null
}

function parseOsc(input: string, start: number): { token: TerminalQueryToken; next: number } | 'incomplete' {
  // start points at the byte after `ESC ]`.
  for (let i = start; i < input.length; i += 1) {
    if (input[i] === ESC && i + 1 >= input.length) {
      return 'incomplete'
    }
    const term = oscTerminatorAt(input, i)
    if (term) {
      const content = input.slice(start, term.contentEnd)
      const semi = content.indexOf(';')
      const idText = semi === -1 ? content : content.slice(0, semi)
      const body = semi === -1 ? '' : content.slice(semi + 1)
      const id = /^\d+$/.test(idText) ? Number.parseInt(idText, 10) : Number.NaN
      return { token: { kind: 'osc', id, body }, next: term.end }
    }
  }
  return 'incomplete'
}

function parseDcs(input: string, start: number): { token: TerminalQueryToken; next: number } | 'incomplete' {
  // start points at the byte after `ESC P`. DCS is ST-terminated (ESC \).
  const terminator = input.indexOf(`${ESC}\\`, start)
  if (terminator === -1) {
    return 'incomplete'
  }
  return { token: { kind: 'dcs', body: input.slice(start, terminator) }, next: terminator + 2 }
}

/**
 * Walk `input`, invoking `visit` once per complete query-relevant sequence.
 * Returns the trailing partial sequence to prepend to the next chunk.
 */
export function scanTerminalQuerySequences(
  input: string,
  visit: (token: TerminalQueryToken) => void
): { pending: string } {
  let offset = 0
  while (offset < input.length) {
    const escIndex = input.indexOf(ESC, offset)
    if (escIndex === -1) {
      return { pending: '' }
    }
    if (escIndex + 1 >= input.length) {
      return { pending: boundedPending(input, escIndex) }
    }
    const kind = input[escIndex + 1]
    let result: { token: TerminalQueryToken; next: number } | 'incomplete'
    if (kind === '[') {
      result = parseCsi(input, escIndex + 2)
    } else if (kind === ']') {
      result = parseOsc(input, escIndex + 2)
    } else if (kind === 'P') {
      result = parseDcs(input, escIndex + 2)
    } else if (kind === 'c') {
      // RIS full reset — clears tracked mode/kitty/margin state.
      visit({ kind: 'ris' })
      offset = escIndex + 2
      continue
    } else {
      // Some other ESC (charset select, cursor save, …): not a query. Skip the
      // introducer and resume — the trailing byte re-scans harmlessly.
      offset = escIndex + 1
      continue
    }
    if (result === 'incomplete') {
      return { pending: boundedPending(input, escIndex) }
    }
    // Skip a malformed CSI (no final byte); every other token is a real query.
    if (result.token.kind !== 'csi' || result.token.final !== '') {
      visit(result.token)
    }
    offset = result.next
  }
  return { pending: '' }
}

function boundedPending(input: string, start: number): string {
  const tail = input.slice(start)
  return tail.length > MAX_PENDING_CHARS ? '' : tail
}
