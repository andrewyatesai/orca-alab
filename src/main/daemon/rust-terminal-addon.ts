import { createRequire } from 'node:module'
import { existsSync } from 'node:fs'
import type { TerminalOscLinkRange } from '../../shared/terminal-osc-link-ranges'
import { isPackagedElectronProcess, orcaNodeAddonCandidatePaths } from './orca-node-addon-paths'

// Typed surface of the napi addon built from native/orca-node (the Rust
// `orca_terminal::HeadlessTerminal`). Node-API is ABI-stable, so the same
// .node loads in both plain Node and Electron without an electron-rebuild.

export type RustHeadlessTerminalHandle = {
  write(data: Buffer): void
  resize(cols: number, rows: number): void
  snapshot(): string[]
  scrollbackLen(): number
  clearScrollback(): void
  cwd(): string | null
  cursor(): number[]
  mouseTracking(): string
  sgrMouse(): boolean
  sgrPixels(): boolean
  isAlternateScreen(): boolean
  bracketedPaste(): boolean
  applicationCursor(): boolean
  /** Drop the native engine now (grid + scrollback) instead of on GC finalize. */
  dispose(): void
  /** Window title (OSC 0/2), or null when unset. */
  title(): string | null
  /** Replayable ANSI: `scrollbackRows` caps the prepended history (omit = all,
   *  0 = viewport-only). */
  serializeAnsi(scrollbackRows?: number): string
  /** Scrollback history only; `maxRows` caps to the most-recent N lines. */
  serializeScrollbackAnsi(maxRows?: number): string
  /** OSC-8 hyperlink ranges over the serialized window (matches the renderer's
   *  `TerminalOscLinkRange`; `endCol` exclusive). */
  oscLinkRanges(scrollbackRows?: number): TerminalOscLinkRange[]
  /** E-5 federated search over history + visible grid: newest-first summaries,
   *  the true total, and the truncation honesty flag. Invalid regex = zero
   *  matches. `cutoffRow` keeps only rows strictly older than it. `originRow`
   *  (fed §2.4; absent on pre-Wave-5 addons — feature-detect) is the stable
   *  absolute row of retained index 0 in the same settled state as the
   *  matches, so `originRow + absRow` is an eviction-stable host row. */
  searchScrollback(
    query: string,
    caseSensitive?: boolean,
    regex?: boolean,
    maxMatches?: number,
    cutoffRow?: number
  ): {
    matches: { absRow: number; col: number; len: number; line: string }[]
    total: number
    incomplete: boolean
    originRow?: number
  }
  /** Context lines around an absolute row, clamped to retained content.
   *  `originRow`: same stable-row contract as `searchScrollback`. */
  searchContext(
    absRow: number,
    before: number,
    after: number
  ): { lines: string[]; firstAbsRow: number; originRow?: number }
  /** Stable absolute row of retained history index 0 (fed §2.4 remote wire):
   *  monotonic across eviction/clear, settled before read. Absent on
   *  pre-Wave-5 addons — callers must feature-detect. */
  retainedOriginRow?(): number
}

export type RustHeadlessTerminalCtor = new (
  cols: number,
  rows: number,
  scrollback?: number
) => RustHeadlessTerminalHandle

export type RustTerminalBinding = {
  HeadlessTerminal: RustHeadlessTerminalCtor
  engine(): string
}

function candidatePaths(): string[] {
  return orcaNodeAddonCandidatePaths({
    override: process.env.ORCA_RUST_TERMINAL_ADDON,
    // Why: packaged builds must never probe cwd — a stale dev addon under the
    // launch directory would silently replace the shipped engine.
    isPackaged: isPackagedElectronProcess(),
    cwd: process.cwd(),
    // resourcesPath is Electron-only, so read it defensively rather than via
    // the global type.
    resourcesPath: (process as { resourcesPath?: string }).resourcesPath
  })
}

let cached: RustTerminalBinding | null | undefined
let failureDetail: string[] = []

/** Why the last `loadRustTerminalBinding()` returned null, one entry per
 *  candidate path. Empty until a load has been attempted and failed. */
export function rustTerminalLoadFailures(): string[] {
  return failureDetail
}

/** Load the Rust terminal addon, or return null if it is unavailable or fails
 *  to load. Never throws itself, but there is NO fallback engine — callers
 *  treat null as a fatal build/packaging fault, using
 *  `rustTerminalLoadFailures()` for the per-candidate causes. */
export function loadRustTerminalBinding(): RustTerminalBinding | null {
  if (cached !== undefined) {
    return cached
  }
  const req = createRequire(import.meta.url)
  const failures: string[] = []
  for (const path of candidatePaths()) {
    if (!existsSync(path)) {
      failures.push(`${path}: not found`)
      continue
    }
    try {
      const binding = req(path) as RustTerminalBinding
      if (binding && typeof binding.HeadlessTerminal === 'function') {
        cached = binding
        return cached
      }
      failures.push(`${path}: loaded but exports no HeadlessTerminal constructor`)
    } catch (error) {
      // Keep the real cause (e.g. an ABI/NODE_MODULE_VERSION mismatch) so the
      // caller's fatal error names it instead of a generic 'failed to load'.
      failures.push(`${path}: ${error instanceof Error ? error.message : String(error)}`)
    }
  }
  failureDetail = failures
  cached = null
  return cached
}
