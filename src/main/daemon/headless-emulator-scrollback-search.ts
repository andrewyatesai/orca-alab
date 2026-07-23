// Fed §2.4 host-side search over a runtime emulator's engine handle: the
// stable-row conversion (originRow + retained index) between the napi
// kernel's retained-relative rows and the wire's eviction-stable host rows.
// Split from headless-emulator.ts so the emulator class stays within its
// line budget; the emulator wraps these in its panic-containment engineCall.
import type { RustHeadlessTerminalHandle } from './rust-terminal-addon'

export type EmulatorScrollbackSearchQuery = {
  query: string
  caseSensitive?: boolean
  regex?: boolean
  maxMatches?: number
}

export type EmulatorScrollbackSearchOutcome = {
  matches: { hostRow: number; col: number; len: number; line: string }[]
  total: number
  incomplete: boolean
  originRow: number
}

export type EmulatorSearchContextWindow = {
  lines: string[]
  firstHostRow: number
}

/** Null when the loaded addon predates the search surface (feature-detect —
 *  the caller degrades to source-unavailable, never an error). */
export function searchEmulatorScrollback(
  term: RustHeadlessTerminalHandle,
  opts: { query: string; caseSensitive?: boolean; regex?: boolean; maxMatches?: number }
): EmulatorScrollbackSearchOutcome | null {
  if (typeof term.searchScrollback !== 'function') {
    return null
  }
  const outcome = term.searchScrollback(
    opts.query,
    opts.caseSensitive === true,
    opts.regex === true,
    opts.maxMatches,
    undefined
  )
  // Pre-Wave-5 addon: no stable origin — matches would be un-remappable.
  if (typeof outcome.originRow !== 'number') {
    return null
  }
  const originRow = outcome.originRow
  return {
    matches: outcome.matches.map((m) => ({
      hostRow: originRow + m.absRow,
      col: m.col,
      len: m.len,
      line: m.line
    })),
    total: outcome.total,
    incomplete: outcome.incomplete,
    originRow
  }
}

/** Context window around a STABLE host row; empty lines when the row is no
 *  longer retained. Null when the addon predates the surface. */
export function emulatorSearchContext(
  term: RustHeadlessTerminalHandle,
  hostRow: number,
  before: number,
  after: number
): EmulatorSearchContextWindow | null {
  const readOrigin = term.retainedOriginRow?.bind(term)
  if (typeof term.searchContext !== 'function' || !readOrigin) {
    return null
  }
  // Settle + read origin FIRST so the retained-relative row we ask for is
  // computed in the same coordinate state the engine answers in.
  const originRow = readOrigin()
  const relative = hostRow - originRow
  if (relative < 0) {
    return { lines: [], firstHostRow: hostRow }
  }
  const window = term.searchContext(relative, before, after)
  const windowOrigin = typeof window.originRow === 'number' ? window.originRow : originRow
  return { lines: window.lines, firstHostRow: windowOrigin + window.firstAbsRow }
}
