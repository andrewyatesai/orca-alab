// In-process E-5 search over STORED terminal content (fed design §2.2, Wave-4
// 4E parked-adapter groundwork): parked panes without a daemon session keep
// their scrollback in the main-side snapshot store, and the federation layer
// must search that content without waking any renderer engine. The stored ANSI
// replays through the napi `HeadlessTerminal` (the same Rust kernel as the
// daemon RPCs — ANSI stripped by headless parse, never a TS regex strip).

import { loadRustTerminalBinding, type RustHeadlessTerminalHandle } from './rust-terminal-addon'
import type { DaemonSearchMatch, DaemonSearchContextWindow } from './daemon-session-search'

export type StoredScrollbackSearchOutcome = {
  matches: DaemonSearchMatch[]
  total: number
  incomplete: boolean
}

export type StoredScrollbackContent = {
  cols: number
  rows: number
  /** Retention for the transient replay; defaults to the policy ceiling so a
   *  large stored snapshot keeps its full searchable depth. */
  scrollbackRows?: number
  /** Stored ANSI in replay order (e.g. scrollbackAnsi then snapshotAnsi). */
  chunks: string[]
}

// Why 50k: matches the daemon replay path — the deepest retention the
// scrollback policy allows, so stored history is never silently truncated.
const REPLAY_SCROLLBACK_ROWS = 50_000

function withTransientReplay<T>(
  content: StoredScrollbackContent,
  f: (term: RustHeadlessTerminalHandle) => T
): T | null {
  const binding = loadRustTerminalBinding()
  if (!binding) {
    // No fallback engine by policy — the caller reports the source unavailable.
    return null
  }
  const term = new binding.HeadlessTerminal(
    content.cols,
    content.rows,
    content.scrollbackRows ?? REPLAY_SCROLLBACK_ROWS
  )
  try {
    for (const chunk of content.chunks) {
      term.write(Buffer.from(chunk, 'utf8'))
    }
    return f(term)
  } finally {
    // Why: a transient replay engine holds a multi-MB grid — free it now, not
    // on GC finalize (a federated fan-out creates one per parked pane).
    term.dispose()
  }
}

/** Search stored parked-pane content in the main process. Null when the native
 *  engine is unavailable (fatal build fault elsewhere — degrade, don't throw). */
export function searchStoredScrollback(
  content: StoredScrollbackContent,
  query: { query: string; caseSensitive?: boolean; regex?: boolean; maxMatches?: number }
): StoredScrollbackSearchOutcome | null {
  return withTransientReplay(content, (term) =>
    term.searchScrollback(
      query.query,
      query.caseSensitive ?? false,
      query.regex ?? false,
      query.maxMatches ?? 50
    )
  )
}

/** Context window around a match in stored parked-pane content. */
export function storedScrollbackSearchContext(
  content: StoredScrollbackContent,
  opts: { absRow: number; before?: number; after?: number }
): DaemonSearchContextWindow | null {
  return withTransientReplay(content, (term) =>
    term.searchContext(opts.absRow, opts.before ?? 20, opts.after ?? 20)
  )
}
