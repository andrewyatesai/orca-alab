import {
  loadRustTerminalBinding,
  rustTerminalLoadFailures,
  type RustHeadlessTerminalHandle
} from './rust-terminal-addon'
import { createPrivateModeScanner } from './private-mode-scan'
import { TerminalKittyKeyboardModeTracker } from '../../shared/terminal-kitty-keyboard-mode-tracker'
import { advancePartialEscapeTail } from '../../shared/terminal-partial-escape-tail'
import { buildRehydrateSequences } from './terminal-mode-rehydrate-sequences'
import { mergeRestoredOscLinks } from './terminal-osc-link-merge'
import { TerminalOscCwdTitleScanner } from './terminal-osc-cwd-title-scanner'
import {
  createTerminalModelQueryResponder,
  type TerminalModelQueryResponder
} from './terminal-model-query-responder'
import type { TerminalSnapshot, TerminalModes } from './types'
import type { TerminalViewAttributes } from '../../shared/terminal-view-attributes'
import type { TerminalOscLinkRange } from '../../shared/terminal-osc-link-ranges'

export type HeadlessEmulatorOptions = {
  cols: number
  rows: number
  scrollback?: number
  /** Query-authority reply sink (terminal-query-authority.md). When set, this
   *  emulator answers terminal queries (DA/DSR/CPR/DECRQM/DECRQSS/XTVERSION/
   *  kitty/OSC-color) for chunks flagged `forwardQueryReplies`. The daemon
   *  Session emulator MUST NOT pass this — it stays write-only. */
  onQueryReply?: (reply: string) => void
  pathFlavor?: 'posix' | 'win32'
  remotePosixFileUriAuthority?: boolean
  wslDistro?: string
}

export type HeadlessEmulatorWriteOptions = {
  /** Reply ownership captured at ingestion for this exact chunk. Default false
   *  is the main-side replay guard: seed/hydration/snapshot writes never
   *  forward replies (a query embedded in replayed bytes answers no one). */
  forwardQueryReplies?: boolean
}

const DEFAULT_SCROLLBACK = 5000

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
 * Query authority (terminal-query-authority.md): when constructed with an
 * `onQueryReply` sink, this emulator ALSO answers terminal queries for
 * chunks flagged `forwardQueryReplies` — the renderer pushes its view
 * attributes and the emulator scans the byte stream for DA/DSR/CPR/DECRQM/
 * DECRQSS/XTVERSION/kitty/OSC-color queries, answering from aterm engine
 * state + the pushed attributes. This is strictly more capable than the old
 * "renderer is the only responder" stance: it answers for parked/hidden/SSH/
 * cold panes with no live renderer attached. The daemon Session emulator
 * passes no sink and stays write-only (it must never race the renderer's
 * reply). aterm's headless engine emits no replies of its own, so the reply
 * grammar lives in TerminalModelQueryResponder, not the native addon.
 */
export class HeadlessEmulator {
  private term: RustHeadlessTerminalHandle
  private cols: number
  private rows: number
  // OSC-7 cwd + OSC 0/2 title scanning, engine-independent so Windows-path
  // normalisation and split-sequence tolerance are preserved. Path flavor must
  // be the PTY host's, not this process's (an SSH/remote PTY differs, #7134).
  private readonly oscText: TerminalOscCwdTitleScanner
  // DECSET mouse-mode tracking scans the raw stream (engine-independent) so
  // 8-bit C1 CSI + split sequences match the former emulator exactly.
  private privateModes = createPrivateModeScanner()
  // Kitty keyboard flags are tracked engine-independently from the raw stream:
  // aterm's headless napi doesn't expose them, and they must survive into
  // snapshots (serialize doesn't carry kitty state) for the query responder's
  // re-seed. Alt-screen keeps its own flag set (the tracker mirrors that).
  private kittyKeyboard = new TerminalKittyKeyboardModeTracker()
  // Why: a chunk ending mid-escape leaves the sequence in the parser, not the
  // grid, so serialize() drops it and the next chunk's continuation renders
  // literal after a restore (Bug E / #7329). Tracked engine-independently at
  // the ingest boundary and shipped out-of-band as pendingEscapeTailAnsi.
  private partialEscapeTail = ''
  private restoredOscLinks: TerminalOscLinkRange[] = []
  private disposed = false
  // Set when a native engine call threw (a Rust panic surfaced as a JS exception
  // via catch_unwind). The engine state is untrustworthy after a panic, so every
  // later engine call is skipped — this session degrades to scan-only state and
  // empty snapshots instead of the panic killing the whole daemon.
  private failed = false
  // Query-authority (terminal-query-authority.md). onQueryReply is read at
  // reply time so disableQueryReplyForwarding can mute a respawn-reused id.
  private onQueryReply: ((reply: string) => void) | null
  private queryResponder: TerminalModelQueryResponder | null = null

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
    this.oscText = new TerminalOscCwdTitleScanner({
      pathFlavor: opts.pathFlavor,
      remotePosixAuthority: opts.remotePosixFileUriAuthority === true,
      wslDistro: opts.wslDistro
    })
    this.cols = opts.cols
    this.rows = opts.rows
    this.term = new binding.HeadlessTerminal(
      opts.cols,
      opts.rows,
      opts.scrollback ?? DEFAULT_SCROLLBACK
    )
    this.onQueryReply = opts.onQueryReply ?? null
    // Only the runtime per-PTY emulators pass a sink; the daemon Session
    // emulator omits it and never builds a responder (stays write-only).
    if (this.onQueryReply) {
      this.ensureQueryResponder()
    }
  }

  /** Build the query responder lazily so the daemon Session emulator (no
   *  onQueryReply, no view-attr responder, no ConPTY override) never carries
   *  one. Wired to read onQueryReply + engine state at reply time. */
  private ensureQueryResponder(): TerminalModelQueryResponder {
    if (!this.queryResponder) {
      this.queryResponder = createTerminalModelQueryResponder({
        emitReply: (reply) => this.onQueryReply?.(reply),
        getCursor: () => this.readCursor(),
        getRows: () => this.rows
      })
    }
    return this.queryResponder
  }

  private readCursor(): [number, number] {
    return this.engineCall(
      'cursor',
      () => {
        const [row, col] = this.term.cursor()
        return [row, col] as [number, number]
      },
      () => [0, 0]
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

  write(data: string, opts: HeadlessEmulatorWriteOptions = {}): Promise<void> {
    if (!this.disposed) {
      this.writeBytes(data, opts.forwardQueryReplies === true)
    }
    return Promise.resolve()
  }

  /** Synchronous write for cold-restore log replay. aterm parses bytes
   *  synchronously; false only when the bytes could not be applied
   *  (disposed, or the engine poisoned itself on this/an earlier write).
   *  Never forwards query replies — it is a seed/replay path. */
  writeSync(data: string): boolean {
    if (this.disposed) {
      return false
    }
    this.writeBytes(data, false)
    return !this.failed
  }

  private writeBytes(data: string, forwardQueryReplies: boolean): void {
    // The OSC/mode scans are engine-independent — keep them current even after a
    // poison so the degraded snapshot still reports honest cwd/title/modes.
    this.oscText.scan(data)
    // aterm's parser consumes bytes; re-encode the daemon's decoded string.
    // Valid UTF-8 round-trips exactly. aterm writes synchronously.
    this.engineCall(
      'write',
      () => this.term.write(Buffer.from(data, 'utf8')),
      () => undefined
    )
    this.privateModes.scan(data)
    this.kittyKeyboard.scan(data)
    // Advance after the parse so the tracked tail reflects the same bytes the
    // grid does; only the trailing unparsed partial sequence is retained.
    this.partialEscapeTail = advancePartialEscapeTail(this.partialEscapeTail, data)
    // After the engine parse so CPR reads the post-write cursor; state tracking
    // (OSC SET overrides, mode flags) runs even when replies are gated off.
    this.queryResponder?.ingest(data, forwardQueryReplies)
  }

  /** Query-authority reply sink for chunks flagged `forwardQueryReplies`.
   *  When set, DA/DSR/CPR/DECRQM/DECRQSS/XTVERSION/kitty/OSC-color queries are
   *  answered; the OSC-color + ?996n family also needs a view-attribute
   *  responder installed (colors stay silent until the first renderer push). */
  installViewAttributeResponder(getter: () => TerminalViewAttributes | null): void {
    this.ensureQueryResponder().setViewAttributesGetter(getter)
  }

  /** Store the latest renderer view-attribute push: cursor style/blink feed
   *  DECRQSS DECSCUSR / DECRQM ?12, and the per-PTY OSC color overrides reset
   *  (a theme apply overwrites mutated colors on visible panes too). */
  applyPushedViewAttributes(attributes: TerminalViewAttributes): void {
    if (this.disposed) {
      return
    }
    this.queryResponder?.applyPushedViewAttributes(attributes)
  }

  /** ConPTY 1.22+ blocks at spawn on DA1 and answers it itself with a
   *  different identity; override the DA1 reply to `CSI ?61;4c`. Idempotent. */
  installConptyPrimaryDeviceAttributesOverride(): void {
    this.ensureQueryResponder().enableConptyDa1Override()
  }

  /** ConPTY swallows the ESC of an OSC 10/11/12 reply written as PTY input and
   *  echoes the printable remainder into the prompt (#6975); mute those
   *  reports for native-Windows-ConPTY PTYs. Idempotent. */
  installConptyOscColorReplySuppression(): void {
    this.ensureQueryResponder().enableConptyOscColorReplySuppression()
  }

  /** Re-seed persisted kitty-keyboard flags via the same `CSI = flags ; 1 u`
   *  parse a live push uses, so hidden `CSI ? u` reports them instead of ?0u.
   *  Routed as an UNFLAGGED write — outside any forwarding window it answers
   *  no one — mirroring the snapshot replay guard. */
  applyKittyKeyboardFlags(flags: number): Promise<void> {
    if (!Number.isInteger(flags) || flags <= 0) {
      return Promise.resolve()
    }
    return this.write(`\x1b[=${flags};1u`)
  }

  /** Permanently mute the reply sink at PTY teardown: queued writeChain links
   *  may still parse after dispose, and daemon respawns reuse session ids — a
   *  late reply must never reach a successor PTY under this id. */
  disableQueryReplyForwarding(): void {
    this.onQueryReply = null
  }

  resize(cols: number, rows: number): void {
    if (this.disposed) {
      return
    }
    this.cols = cols
    this.rows = rows
    this.restoredOscLinks = []
    // A resize resets the scroll region to the full viewport (DECSTBM), so the
    // query responder's margin cache must follow.
    this.queryResponder?.onResize()
    this.engineCall(
      'resize',
      () => this.term.resize(cols, rows),
      () => undefined
    )
  }

  // Why: Session.resize applies this emulator and the node-pty subprocess
  // together behind the same dead/invalid-size gate, so the emulator's dims are
  // an accurate proxy for the size the child actually took — and stay stale
  // when a resize is dropped, which is exactly the drop the renderer must detect.
  getAppliedSize(): { cols: number; rows: number } {
    // this.cols/this.rows advance only through resize() (Session gates dead/
    // invalid sizes before calling it), so they are the engine's applied dims.
    return { cols: this.cols, rows: this.rows }
  }

  getSnapshot(opts: { scrollbackRows?: number } = {}): TerminalSnapshot {
    const modes = this.getModes()
    const scrollbackRows = opts.scrollbackRows
    // Why: written LAST by the restorer (after any reset) so the next live chunk
    // completes this dangling sequence instead of rendering it literally
    // (Bug E / #7329). Its bytes are already counted by the snapshot seq.
    const pendingEscapeTail =
      this.partialEscapeTail.length > 0 ? { pendingEscapeTailAnsi: this.partialEscapeTail } : {}
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
        rehydrateSequences: buildRehydrateSequences(modes),
        cwd: this.oscText.cwd,
        modes,
        cols: this.cols,
        rows: this.rows,
        scrollbackLines: this.term.scrollbackLen(),
        lastTitle: this.oscText.lastTitle ?? undefined,
        ...pendingEscapeTail
      }),
      // Poisoned engine: no replayable buffer to offer, but the scanned state
      // (cwd/modes/title/partial-tail) is still honest, so reconnect/rehydrate
      // keep working.
      () => ({
        snapshotAnsi: '',
        scrollbackAnsi: '',
        oscLinks: this.restoredOscLinks.slice(),
        rehydrateSequences: buildRehydrateSequences(modes),
        cwd: this.oscText.cwd,
        modes,
        cols: this.cols,
        rows: this.rows,
        scrollbackLines: 0,
        lastTitle: this.oscText.lastTitle ?? undefined,
        ...pendingEscapeTail
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

  /** The dangling incomplete escape at the current stream position (empty
   *  when none). Scan-authority handoffs seed the other side's fact scanners
   *  with it so a sequence split across the handoff neither mints a phantom
   *  bell (unseen OSC terminator) nor loses its fact. Contains no complete
   *  sequence by construction, so seeding can never double-fire. */
  get partialEscapeTailAnsi(): string {
    return this.partialEscapeTail
  }

  /** Why: PSReadLine's Ctrl+L repaint is only safe at an empty prompt — with
   *  pending input it re-renders at a cached buffer row that ConPTY's fixed
   *  viewport doesn't track, painting the input well below the prompt. The
   *  cursor line counts as an empty prompt when everything before the cursor
   *  ends with a single '>' and nothing follows it ('>>' is PowerShell's
   *  continuation prompt, i.e. a multiline edit in flight). */
  isCursorOnEmptyPromptLine(): boolean {
    // aterm owns the grid: read the cursor row via the facade (cursor()/snapshot())
    // rather than the removed xterm buffer API. Same '>' vs '>>' heuristic as upstream.
    return this.engineCall(
      'isCursorOnEmptyPromptLine',
      () => {
        const [row, col] = this.term.cursor()
        const line = this.term.snapshot()[row] ?? ''
        const upToCursor = line.slice(0, col).trimEnd()
        const fullLine = line.trimEnd()
        return fullLine === upToCursor && upToCursor.endsWith('>') && !upToCursor.endsWith('>>')
      },
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
    return this.oscText.cwd
  }

  setCwd(cwd: string | null): void {
    this.oscText.cwd = cwd
  }

  setLastTitle(title: string): void {
    this.oscText.lastTitle = title
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
    // (aterm retains hyperlink spans on scroll) — merged with checkpoint-
    // restored links so a restored buffer keeps clickable links.
    return mergeRestoredOscLinks(
      this.term.oscLinkRanges(scrollbackRows),
      this.restoredOscLinks,
      this.cols
    )
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
      sgrMousePixelsMode: this.privateModes.sgrMousePixelsMode(),
      // Engine-independent (aterm napi doesn't expose kitty flags); the query
      // responder re-seeds from this. Deliberately NOT added to rehydrate — the
      // renderer re-negotiates the protocol on reconnect.
      kittyKeyboardFlags: this.kittyKeyboard.flags
    }
  }
}
