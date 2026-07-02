import {
  loadRustTerminalBinding,
  rustTerminalLoadFailures,
  type RustHeadlessTerminalHandle
} from './rust-terminal-addon'
import { extractLastOscTitle } from '../../shared/agent-detection'
import { extractOscScanTail, scanOsc7Uris } from './osc7-uri-extraction'
import { parseFileUriPath } from './osc7-file-uri'
import { createPrivateModeScanner } from './private-mode-scan'
import type { TerminalSnapshot, TerminalModes } from './types'
import type { TerminalOscLinkRange } from '../../shared/terminal-osc-link-ranges'

export type HeadlessEmulatorOptions = {
  cols: number
  rows: number
  scrollback?: number
}

const DEFAULT_SCROLLBACK = 5000
const OSC_SCAN_TAIL_LIMIT = 4096

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
 * answer DA/DSR/OSC-color queries — the renderer's aterm engine is the
 * authoritative responder. aterm's headless engine emits no replies.
 */
export class HeadlessEmulator {
  private term: RustHeadlessTerminalHandle
  private cols: number
  private rows: number
  private cwd: string | null = null
  private lastTitle: string | null = null
  private oscScanTail = ''
  // DECSET mouse-mode tracking scans the raw stream (engine-independent) so
  // 8-bit C1 CSI + split sequences match the former emulator exactly.
  private privateModes = createPrivateModeScanner()
  private restoredOscLinks: TerminalOscLinkRange[] = []
  private disposed = false
  // Set when a native engine call threw (a Rust panic surfaced as a JS exception
  // via catch_unwind). The engine state is untrustworthy after a panic, so every
  // later engine call is skipped — this session degrades to scan-only state and
  // empty snapshots instead of the panic killing the whole daemon.
  private failed = false

  constructor(opts: HeadlessEmulatorOptions) {
    const binding = loadRustTerminalBinding()
    if (!binding) {
      // No fallback: aterm is the sole headless engine. A missing addon is a
      // build/packaging fault that must surface, not degrade quietly — with
      // the per-candidate causes so e.g. an ABI mismatch is diagnosable.
      throw new Error(
        '[orca] aterm terminal addon (orca_node.node) failed to load — run ' +
          '`pnpm build:terminal-addon` for dev, or check that packaging ships it. ' +
          `Candidates: ${rustTerminalLoadFailures().join('; ') || 'none probed'}`
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

  /** Run one native engine call with panic containment: catch_unwind surfaces a
   *  Rust panic as a JS exception, so catch it here, poison this emulator (one
   *  loud log), and return the degraded fallback. The daemon and its other
   *  sessions keep running; the respawn/snapshot machinery recovers this one. */
  private engineCall<T>(op: string, call: () => T, fallback: () => T): T {
    if (this.failed) {
      return fallback()
    }
    try {
      return call()
    } catch (error) {
      this.failed = true
      console.error(
        `[orca] aterm terminal engine ${op} failed — poisoning this session's emulator ` +
          '(scan-only state from here; other sessions unaffected):',
        error
      )
      return fallback()
    }
  }

  write(data: string): Promise<void> {
    if (!this.disposed) {
      this.writeBytes(data)
    }
    return Promise.resolve()
  }

  /** Synchronous write for cold-restore log replay. aterm parses bytes
   *  synchronously; false only when the bytes could not be applied
   *  (disposed, or the engine poisoned itself on this/an earlier write). */
  writeSync(data: string): boolean {
    if (this.disposed) {
      return false
    }
    this.writeBytes(data)
    return !this.failed
  }

  private writeBytes(data: string): void {
    // The OSC/mode scans are engine-independent — keep them current even after a
    // poison so the degraded snapshot still reports honest cwd/title/modes.
    this.scanInputForOscState(data)
    // aterm's parser consumes bytes; re-encode the daemon's decoded string.
    // Valid UTF-8 round-trips exactly. aterm writes synchronously.
    this.engineCall(
      'write',
      () => this.term.write(Buffer.from(data, 'utf8')),
      () => undefined
    )
    this.privateModes.scan(data)
  }

  resize(cols: number, rows: number): void {
    if (this.disposed) {
      return
    }
    this.cols = cols
    this.rows = rows
    this.restoredOscLinks = []
    this.engineCall(
      'resize',
      () => this.term.resize(cols, rows),
      () => undefined
    )
  }

  getSnapshot(opts: { scrollbackRows?: number } = {}): TerminalSnapshot {
    const modes = this.getModes()
    const scrollbackRows = opts.scrollbackRows
    return this.engineCall(
      'serialize',
      () => ({
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
      }),
      // Poisoned engine: no replayable buffer to offer, but the scanned state
      // (cwd/modes/title) is still honest, so reconnect/rehydrate keep working.
      () => ({
        snapshotAnsi: '',
        scrollbackAnsi: '',
        oscLinks: this.restoredOscLinks.slice(),
        rehydrateSequences: this.buildRehydrateSequences(modes),
        cwd: this.cwd,
        modes,
        cols: this.cols,
        rows: this.rows,
        scrollbackLines: 0,
        lastTitle: this.lastTitle ?? undefined
      })
    )
  }

  get isAlternateScreen(): boolean {
    return this.engineCall(
      'isAlternateScreen',
      () => this.term.isAlternateScreen(),
      () => false
    )
  }

  getVisibleLines(): string[] {
    return this.engineCall(
      'snapshot',
      () => this.term.snapshot(),
      () => []
    )
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
    this.engineCall(
      'clearScrollback',
      () => {
        const [cursorRow, cursorCol] = this.term.cursor()
        const line = this.term.snapshot()[cursorRow] ?? ''
        this.term.clearScrollback()
        this.term.write(Buffer.from('\x1b[H\x1b[2J', 'utf8'))
        if (line) {
          this.term.write(Buffer.from(line, 'utf8'))
        }
        this.term.write(Buffer.from(`\x1b[1;${cursorCol + 1}H`, 'utf8'))
      },
      () => undefined
    )
  }

  dispose(): void {
    this.disposed = true
    try {
      // Free the native grid/scrollback now: the daemon churns through many
      // sessions and GC finalization of a multi-MB handle is unbounded.
      this.term.dispose()
    } catch {
      // A poisoned engine may throw even here; GC still reclaims the handle.
    }
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
    // Screen/input modes come straight from the aterm engine (false once
    // poisoned)…
    const engineModes = this.engineCall(
      'modes',
      () => ({
        bracketedPaste: this.term.bracketedPaste(),
        applicationCursor: this.term.applicationCursor(),
        alternateScreen: this.term.isAlternateScreen()
      }),
      () => ({ bracketedPaste: false, applicationCursor: false, alternateScreen: false })
    )
    // …mouse modes come from the raw-stream scanner (see privateModes).
    const mouseTrackingMode = this.privateModes.mouseTrackingMode()
    return {
      ...engineModes,
      mouseTracking: mouseTrackingMode !== 'none',
      mouseTrackingMode,
      sgrMouseMode: this.privateModes.sgrMouseMode(),
      sgrMousePixelsMode: this.privateModes.sgrMousePixelsMode()
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
