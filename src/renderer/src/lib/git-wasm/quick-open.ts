// Quick Open ranking, driven by the Rust orca-text core in the orca-git wasm
// module. The per-keystroke ranking used to be TS (quick-open-search.ts); the
// wasm QuickOpenIndex holds the PREPARED file list inside linear memory, so
// only the query string and the top-N JSON cross the boundary per keystroke —
// benchmarked ~4x faster than the TS scan over 20k paths, with identical
// results (paths and scores).
import { QuickOpenIndex as WasmQuickOpenIndex } from './orca_git_wasm.js'
import { isGitWasmReady } from './git-line-stats'

export const QUICK_OPEN_RESULT_LIMIT = 50

export type QuickOpenSearchResult = {
  path: string
  score: number
}

export type QuickOpenExactMatches = {
  paths: string[]
  basenames: string[]
}

export type QuickOpenRankIndex = {
  /** Top-`limit` fuzzy matches, best first. Empty until the wasm is ready. */
  rank(query: string, limit?: number): QuickOpenSearchResult[]
  /** Exact-path/basename matches for an already-lowercased query. */
  exactMatches(lowerQuery: string): QuickOpenExactMatches
  readonly fileCount: number
}

const EMPTY_EXACT: QuickOpenExactMatches = { paths: [], basenames: [] }

/**
 * Build a prepared ranking index over a worktree file list. Construction is
 * lazy against wasm readiness: until the module initialises (a ~tens-of-ms
 * window at boot), rank/exactMatches return empty and consumers re-render via
 * `subscribeGitWasmReady` — Quick Open surfaces open on user action, well
 * after readiness in practice.
 */
export function createQuickOpenIndex(files: readonly string[]): QuickOpenRankIndex {
  let inner: WasmQuickOpenIndex | null = null
  const getInner = (): WasmQuickOpenIndex | null => {
    if (inner === null && isGitWasmReady()) {
      // NUL-joined: file names cannot contain NUL, so the list crosses the
      // boundary as one string instead of N marshalled values.
      inner = new WasmQuickOpenIndex(files.join('\0'))
    }
    return inner
  }
  return {
    rank(query: string, limit: number = QUICK_OPEN_RESULT_LIMIT): QuickOpenSearchResult[] {
      const index = getInner()
      if (!index) {
        return []
      }
      return JSON.parse(index.rank(query, limit)) as QuickOpenSearchResult[]
    },
    exactMatches(lowerQuery: string): QuickOpenExactMatches {
      const index = getInner()
      if (!index) {
        return EMPTY_EXACT
      }
      return JSON.parse(index.exactMatches(lowerQuery)) as QuickOpenExactMatches
    },
    get fileCount(): number {
      return getInner()?.fileCount() ?? 0
    }
  }
}
