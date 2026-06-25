import { loadRustTerminalBinding, type RustHeadlessTerminalHandle } from './rust-terminal-addon'
import { extractLastOscTitle } from '../../shared/agent-detection'
import { extractOscScanTail, scanOsc7Uris } from './osc7-uri-extraction'
import { parseFileUriPath } from './osc7-file-uri'
import type { TerminalSnapshot, TerminalModes } from './types'
import type { TerminalOscLinkRange } from '../../shared/terminal-osc-link-ranges'

export type HeadlessEmulatorOptions = {
  cols: number
  rows: number
  scrollback?: number
}

const DEFAULT_SCROLLBACK = 5000
const OSC_SCAN_TAIL_LIMIT = 4096
// Why: PTY/SSH chunks can split a long combined DECSET before the final h/l.
// Keep parser state far beyond normal mode lists while still bounding memory.
const PRIVATE_MODE_SCAN_TAIL_LIMIT = 4096
type MouseTrackingMode = NonNullable<TerminalModes['mouseTrackingMode']>

const linkKey = (r: TerminalOscLinkRange): string => `${r.row}:${r.startCol}:${r.endCol}:${r.uri}`

/**
 * Server-side terminal-state emulator (snapshots, cwd, mode flags, OSC-8 links)
 * for snapshot/replay across reconnect and SSH. Backed by the aterm engine via
 * the native addon — the replacement for the former headless xterm engine.
 * aterm owns the VT parser, grid, tiered scrollback, SGR/colour model, and
 * OSC-8 hyperlinks; cwd (OSC-7), window title (OSC 0/2) and mouse modes are
 * scanned here from the raw byte stream (engine-independent) so Orca's
 * Windows-path normalisation, 8-bit C1 handling, and split-sequence tolerance
 * are preserved exactly.
 *
 * Why no query replies: this exists purely for state tracking and MUST NOT
 * answer DA/DSR/OSC-color queries — the renderer's xterm is the authoritative
 * responder. aterm's headless engine emits no replies.
 */
export class HeadlessEmulator {
  private term: RustHeadlessTerminalHandle
  private cols: number
  private rows: number
  private cwd: string | null = null
  private lastTitle: string | null = null
  private oscScanTail = ''
  private privateModeScanTail = ''
  private mouseTrackingMode: MouseTrackingMode = 'none'
  private sgrMouseMode = false
  private sgrMousePixelsMode = false
  private restoredOscLinks: TerminalOscLinkRange[] = []
  private disposed = false

  constructor(opts: HeadlessEmulatorOptions) {
    const binding = loadRustTerminalBinding()
    if (!binding) {
      // No silent xterm fallback: aterm is the sole headless engine. A missing
      // addon is a build/packaging fault that must surface, not degrade quietly.
      throw new Error(
        '[orca] aterm terminal addon (orca_node.node) failed to load — run ' +
          '`pnpm build:terminal-addon` for dev, or check that packaging ships it.'
      )
    }
    this.cols = opts.cols
    this.rows = opts.rows
    this.term = new binding.HeadlessTerminal(
      opts.cols,
      opts.rows,
      opts.scrollback ?? DEFAULT_SCROLLBACK
    )
  }

  write(data: string): Promise<void> {
    if (!this.disposed) {
      this.writeBytes(data)
    }
    return Promise.resolve()
  }

  /** Synchronous write for cold-restore log replay. aterm parses bytes
   *  synchronously, so unlike the old xterm sync path this never fails. */
  writeSync(data: string): boolean {
    if (this.disposed) {
      return false
    }
    this.writeBytes(data)
    return true
  }

  private writeBytes(data: string): void {
    this.scanInputForOscState(data)
    // aterm's parser consumes bytes; re-encode the daemon's decoded string.
    // Valid UTF-8 round-trips exactly. aterm writes synchronously.
    this.term.write(Buffer.from(data, 'utf8'))
    this.scanPrivateModes(data)
  }

  resize(cols: number, rows: number): void {
    if (this.disposed) {
      return
    }
    this.cols = cols
    this.rows = rows
    this.restoredOscLinks = []
    this.term.resize(cols, rows)
  }

  getSnapshot(opts: { scrollbackRows?: number } = {}): TerminalSnapshot {
    const modes = this.getModes()
    const scrollbackRows = opts.scrollbackRows
    return {
      snapshotAnsi: this.term.serializeAnsi(scrollbackRows),
      // SPLIT shape: the visible viewport lives in snapshotAnsi, history in
      // scrollbackAnsi (independent of scrollbackRows). The alt-screen
      // cold-restore path needs this — the alt buffer has no scrollback, so its
      // pre-TUI history is only recoverable here.
      scrollbackAnsi: this.term.serializeScrollbackAnsi(),
      oscLinks: this.collectOscLinks(scrollbackRows),
      rehydrateSequences: this.buildRehydrateSequences(modes),
      cwd: this.cwd,
      modes,
      cols: this.cols,
      rows: this.rows,
      scrollbackLines: this.term.scrollbackLen(),
      lastTitle: this.lastTitle ?? undefined
    }
  }

  get isAlternateScreen(): boolean {
    return this.term.isAlternateScreen()
  }

  getVisibleLines(): string[] {
    return this.term.snapshot()
  }

  getCwd(): string | null {
    return this.cwd
  }

  setCwd(cwd: string | null): void {
    this.cwd = cwd
  }

  setLastTitle(title: string): void {
    this.lastTitle = title
  }

  setRestoredOscLinks(links: TerminalOscLinkRange[] | undefined): void {
    this.restoredOscLinks = links?.slice() ?? []
  }

  clearScrollback(): void {
    this.restoredOscLinks = []
    // Match the former headless xterm clear(): keep the cursor's current line
    // as the new first row, discarding everything above/below it and all
    // scrollback. Orca's "clear" action and cold-restore 'clear' records relied
    // on this semantic, not a bare history drop.
    const [cursorRow, cursorCol] = this.term.cursor()
    const line = this.term.snapshot()[cursorRow] ?? ''
    this.term.clearScrollback()
    this.term.write(Buffer.from('\x1b[H\x1b[2J', 'utf8'))
    if (line) {
      this.term.write(Buffer.from(line, 'utf8'))
    }
    this.term.write(Buffer.from(`\x1b[1;${cursorCol + 1}H`, 'utf8'))
  }

  dispose(): void {
    // The native handle is freed when GC collects it; nothing to release here.
    this.disposed = true
  }

  private collectOscLinks(scrollbackRows?: number): TerminalOscLinkRange[] {
    // Live links come from aterm — both the visible grid and scrollback history
    // (aterm retains hyperlink spans on scroll).
    const live = this.term.oscLinkRanges(scrollbackRows)
    if (this.restoredOscLinks.length === 0) {
      return live
    }
    // Merge checkpoint-restored links with live ones, clamped to the current
    // width and de-duplicated, so a restored buffer keeps clickable links.
    const merged = [...live]
    const seen = new Set(live.map(linkKey))
    for (const link of this.restoredOscLinks) {
      const clamped: TerminalOscLinkRange = {
        ...link,
        startCol: Math.min(link.startCol, this.cols),
        endCol: Math.min(link.endCol, this.cols)
      }
      const key = linkKey(clamped)
      if (!seen.has(key)) {
        seen.add(key)
        merged.push(clamped)
      }
    }
    return merged
  }

  private getModes(): TerminalModes {
    return {
      // Screen/input modes come straight from the aterm engine…
      bracketedPaste: this.term.bracketedPaste(),
      applicationCursor: this.term.applicationCursor(),
      alternateScreen: this.term.isAlternateScreen(),
      // …mouse modes are scanned here so 8-bit C1 CSI and split sequences match
      // the old emulator (aterm does not parse 8-bit C1 controls).
      mouseTracking: this.mouseTrackingMode !== 'none',
      mouseTrackingMode: this.mouseTrackingMode,
      sgrMouseMode: this.sgrMouseMode,
      sgrMousePixelsMode: this.sgrMousePixelsMode
    }
  }

  private scanInputForOscState(data: string): void {
    const oscInput = this.oscScanTail + data
    this.oscScanTail = extractOscScanTail(oscInput, OSC_SCAN_TAIL_LIMIT)
    scanOsc7Uris(oscInput, (uri) => this.parseOsc7Uri(uri))
    const lastTitle = extractLastOscTitle(oscInput)
    if (lastTitle !== null) {
      this.lastTitle = lastTitle
    }
  }

  private parseOsc7Uri(uri: string): void {
    const parsed = parseFileUriPath(uri)
    if (parsed) {
      this.cwd = parsed
    }
  }

  private scanPrivateModes(data: string): void {
    const input = this.privateModeScanTail + data
    this.privateModeScanTail = this.extractPrivateModeScanTail(input)
    // oxlint-disable-next-line no-control-regex -- terminal escape sequences require control chars
    const privateModeRe = /\x1bc|\x1b\[\?([0-9;]+)([hl])|\x9b\?([0-9;]+)([hl])/g
    let match: RegExpExecArray | null
    while ((match = privateModeRe.exec(input)) !== null) {
      if (match[0] === '\x1bc') {
        this.mouseTrackingMode = 'none'
        this.sgrMouseMode = false
        this.sgrMousePixelsMode = false
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
          this.mouseTrackingMode = enabled ? 'x10' : 'none'
        }
        if (param === 1000) {
          this.mouseTrackingMode = enabled ? 'vt200' : 'none'
        }
        if (param === 1002) {
          this.mouseTrackingMode = enabled ? 'drag' : 'none'
        }
        if (param === 1003) {
          this.mouseTrackingMode = enabled ? 'any' : 'none'
        }
        if (param === 1006) {
          this.sgrMouseMode = enabled
          this.sgrMousePixelsMode = false
        }
        if (param === 1016) {
          this.sgrMouseMode = false
          this.sgrMousePixelsMode = enabled
        }
      }
    }
  }

  private extractPrivateModeScanTail(input: string): string {
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
      return this.isIncompletePrivateModeParams(tail.slice(3)) ? tail : ''
    }
    if (tail.startsWith('\x9b?')) {
      return this.isIncompletePrivateModeParams(tail.slice(2)) ? tail : ''
    }
    return ''
  }

  private isIncompletePrivateModeParams(params: string): boolean {
    return /^[0-9;]*$/.test(params)
  }

  private buildRehydrateSequences(modes: TerminalModes): string {
    const seqs: string[] = []
    // Restore screen/input modes on reconnect so a replay re-enters the
    // alternate screen and re-arms paste / app-cursor / mouse reporting.
    if (modes.alternateScreen) {
      seqs.push('\x1b[?1049h')
    }
    if (modes.bracketedPaste) {
      seqs.push('\x1b[?2004h')
    }
    if (modes.applicationCursor) {
      seqs.push('\x1b[?1h')
    }
    switch (modes.mouseTracking ? (modes.mouseTrackingMode ?? 'vt200') : 'none') {
      case 'x10':
        seqs.push('\x1b[?9h')
        break
      case 'vt200':
        seqs.push('\x1b[?1000h')
        break
      case 'drag':
        seqs.push('\x1b[?1002h')
        break
      case 'any':
        seqs.push('\x1b[?1003h')
        break
      case 'none':
        break
    }
    // xterm tracks the mouse protocol and SGR encoding independently, so a
    // snapshot must preserve the encoding even when reporting is off.
    if (modes.sgrMousePixelsMode) {
      seqs.push('\x1b[?1016h')
    } else if (modes.sgrMouseMode) {
      seqs.push('\x1b[?1006h')
    }
    return seqs.join('')
  }
}
