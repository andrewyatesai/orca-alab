import { HeadlessEmulator, type HeadlessEmulatorOptions } from './headless-emulator'
import { loadRustTerminalBinding, type RustHeadlessTerminalHandle } from './rust-terminal-addon'
import type { TerminalSnapshot, TerminalModes } from './types'

// The emulator surface the daemon session depends on. Both the TypeScript
// HeadlessEmulator and the Rust-backed one satisfy it, so the session can use
// either behind the ORCA_RUST_TERMINAL flag.
export type TerminalEmulator = {
  write(data: string): void
  resize(cols: number, rows: number): void
  getSnapshot(): TerminalSnapshot
  getCwd(): string | null
  clearScrollback(): void
  dispose(): void
}

const RUST_MOUSE_MODE: Record<string, NonNullable<TerminalModes['mouseTrackingMode']>> = {
  None: 'none',
  X10: 'x10',
  Normal: 'vt200',
  Button: 'drag',
  Any: 'any'
}

/** Drop-in for the daemon's terminal-state emulator, backed by the Rust
 *  `orca_terminal::HeadlessTerminal`. Renders the same visible grid AND per-cell
 *  SGR attributes as xterm.js across the conformance corpus (tools/conformance —
 *  grid + attribute goldens regenerated from real xterm), and is faster to parse.
 *  Tracks cursor positioning, scroll regions, charsets, the alternate screen, cwd,
 *  and the snapshot mode flags. Selected via the ORCA_RUST_TERMINAL flag. */
class RustHeadlessEmulator implements TerminalEmulator {
  private term: RustHeadlessTerminalHandle
  private cols: number
  private rows: number

  constructor(
    ctor: new (cols: number, rows: number, scrollback?: number) => RustHeadlessTerminalHandle,
    opts: HeadlessEmulatorOptions
  ) {
    this.cols = opts.cols
    this.rows = opts.rows
    this.term = new ctor(opts.cols, opts.rows, opts.scrollback)
  }

  write(data: string): void {
    // The daemon decodes PTY bytes to a (UTF-8) string before this point; the
    // Rust parser consumes bytes, so re-encode. Valid UTF-8 round-trips exactly.
    this.term.write(Buffer.from(data, 'utf8'))
  }

  resize(cols: number, rows: number): void {
    this.cols = cols
    this.rows = rows
    this.term.resize(cols, rows)
  }

  getSnapshot(): TerminalSnapshot {
    const modes = this.getModes()
    return {
      snapshotAnsi: this.term.serializeAnsi(),
      scrollbackAnsi: '',
      rehydrateSequences: this.buildRehydrateSequences(modes),
      cwd: this.term.cwd(),
      modes,
      cols: this.cols,
      rows: this.rows,
      scrollbackLines: this.term.scrollbackLen()
    }
  }

  getCwd(): string | null {
    return this.term.cwd()
  }

  clearScrollback(): void {
    this.term.clearScrollback()
  }

  dispose(): void {
    // The native handle is freed when GC collects it; nothing to release here.
  }

  private getModes(): TerminalModes {
    const mouseTrackingMode = RUST_MOUSE_MODE[this.term.mouseTracking()] ?? 'none'
    return {
      bracketedPaste: this.term.bracketedPaste(),
      applicationCursor: this.term.applicationCursor(),
      alternateScreen: this.term.isAlternateScreen(),
      mouseTracking: mouseTrackingMode !== 'none',
      mouseTrackingMode,
      sgrMouseMode: this.term.sgrMouse(),
      sgrMousePixelsMode: this.term.sgrPixels()
    }
  }

  private buildRehydrateSequences(modes: TerminalModes): string {
    const seqs: string[] = []
    // Restore screen/input modes on reconnect, matching the TS HeadlessEmulator
    // so a replay re-enters the alternate screen and re-arms paste / app-cursor.
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
    if (modes.sgrMousePixelsMode) {
      seqs.push('\x1b[?1016h')
    } else if (modes.sgrMouseMode) {
      seqs.push('\x1b[?1006h')
    }
    return seqs.join('')
  }
}

/** Build the daemon terminal emulator. Returns the Rust-backed engine when
 *  ORCA_RUST_TERMINAL is enabled and the native addon loads; otherwise the
 *  battle-tested TypeScript HeadlessEmulator. Safe by default. */
let loggedSelection = false

function markEngine(engine: string): void {
  // Diagnostic: the daemon runs with stdio 'ignore', so console.log is
  // invisible. When ORCA_ENGINE_MARKER is set, record the selected engine to
  // that file so an E2E harness can prove which emulator went live (and catch a
  // silent TS fallback). Gated on the env var; no effect in normal runs.
  const marker = process.env.ORCA_ENGINE_MARKER
  if (!marker) {
    return
  }
  try {
    require('fs').appendFileSync(marker, `${engine}\n`)
  } catch {
    // diagnostic only — never break session creation
  }
}

export function createHeadlessEmulator(opts: HeadlessEmulatorOptions): TerminalEmulator {
  if (process.env.ORCA_RUST_TERMINAL === '1') {
    const binding = loadRustTerminalBinding()
    if (binding) {
      if (!loggedSelection) {
        loggedSelection = true
        // One-time proof, in the daemon log, that the native engine is live.
        console.log(`[orca] terminal engine: Rust (${binding.engine()}) via ORCA_RUST_TERMINAL`)
      }
      markEngine(`rust:${binding.engine()}`)
      return new RustHeadlessEmulator(binding.HeadlessTerminal, opts)
    }
    if (!loggedSelection) {
      loggedSelection = true
      console.warn(
        '[orca] ORCA_RUST_TERMINAL=1 but the Rust addon did not load; using the TS emulator'
      )
    }
    markEngine('ts-fallback')
    return new HeadlessEmulator(opts)
  }
  markEngine('ts')
  return new HeadlessEmulator(opts)
}
