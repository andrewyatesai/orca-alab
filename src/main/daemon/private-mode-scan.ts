import type { TerminalModes } from './types'

// Why: PTY/SSH chunks can split a long combined DECSET before the final h/l.
// Keep parser state far beyond normal mode lists while still bounding memory.
const PRIVATE_MODE_SCAN_TAIL_LIMIT = 4096

export type MouseTrackingMode = NonNullable<TerminalModes['mouseTrackingMode']>

export type PrivateModeScanner = {
  /** Feed the next raw output chunk (call once per chunk, in arrival order). */
  scan: (data: string) => void
  mouseTrackingMode: () => MouseTrackingMode
  sgrMouseMode: () => boolean
  sgrMousePixelsMode: () => boolean
}

/** Tracks DECSET/DECRST mouse-reporting modes (and RIS resets) by scanning the
 *  raw byte stream, engine-independently: aterm does not parse 8-bit C1 CSI, and
 *  sequences can arrive split across chunks — this scanner preserves the former
 *  emulator's behavior for both. */
export function createPrivateModeScanner(): PrivateModeScanner {
  let scanTail = ''
  let mouseTrackingMode: MouseTrackingMode = 'none'
  let sgrMouseMode = false
  let sgrMousePixelsMode = false

  const isIncompletePrivateModeParams = (params: string): boolean => /^[0-9;]*$/.test(params)

  const extractScanTail = (input: string): string => {
    const start = Math.max(input.lastIndexOf('\x1b'), input.lastIndexOf('\x9b'))
    if (start === -1) {
      return ''
    }
    const tail = input.slice(start)
    if (tail.length > PRIVATE_MODE_SCAN_TAIL_LIMIT) {
      return ''
    }
    if (tail === '\x1b' || tail === '\x1b[' || tail === '\x9b') {
      return tail
    }
    if (tail.startsWith('\x1b[?')) {
      return isIncompletePrivateModeParams(tail.slice(3)) ? tail : ''
    }
    if (tail.startsWith('\x9b?')) {
      return isIncompletePrivateModeParams(tail.slice(2)) ? tail : ''
    }
    return ''
  }

  const scan = (data: string): void => {
    const input = scanTail + data
    scanTail = extractScanTail(input)
    // oxlint-disable-next-line no-control-regex -- terminal escape sequences require control chars
    const privateModeRe = /\x1bc|\x1b\[\?([0-9;]+)([hl])|\x9b\?([0-9;]+)([hl])/g
    let match: RegExpExecArray | null
    while ((match = privateModeRe.exec(input)) !== null) {
      if (match[0] === '\x1bc') {
        mouseTrackingMode = 'none'
        sgrMouseMode = false
        sgrMousePixelsMode = false
        continue
      }
      const params = match[1] ?? match[3]
      const enabled = (match[2] ?? match[4]) === 'h'
      for (const rawParam of params.split(';')) {
        if (rawParam === '') {
          continue
        }
        const param = Number(rawParam)
        if (!Number.isInteger(param)) {
          continue
        }
        if (param === 9) {
          mouseTrackingMode = enabled ? 'x10' : 'none'
        }
        if (param === 1000) {
          mouseTrackingMode = enabled ? 'vt200' : 'none'
        }
        if (param === 1002) {
          mouseTrackingMode = enabled ? 'drag' : 'none'
        }
        if (param === 1003) {
          mouseTrackingMode = enabled ? 'any' : 'none'
        }
        if (param === 1006) {
          sgrMouseMode = enabled
          sgrMousePixelsMode = false
        }
        if (param === 1016) {
          sgrMouseMode = false
          sgrMousePixelsMode = enabled
        }
      }
    }
  }

  return {
    scan,
    mouseTrackingMode: () => mouseTrackingMode,
    sgrMouseMode: () => sgrMouseMode,
    sgrMousePixelsMode: () => sgrMousePixelsMode
  }
}
