// Feature detection for the E-1 federated-summary wasm binding (`search_summary`)
// and the W4A search-index release binding — both land with a future aterm pin, so
// the glue detects them at engine build and degrades honestly when absent (matches
// stay bare triplets; eviction falls back to budgeted-cursor cancel alone).

import type { AtermFederatedMatch } from './aterm-worker-federated-protocol'

/** A decoded `search_summary` result, copied out of wasm memory (the wasm object
 *  is freed before returning so summaries never pin linear memory). */
export type EngineFederatedSummary = {
  /** Newest-first is NOT guaranteed here — the caller orders. */
  matches: AtermFederatedMatch[]
  total: number
  incomplete: boolean
}

/** The wasm-bindgen getter surface the E-1 summary object is expected to expose
 *  (per FEDERATED-SEARCH-DESIGN §3 E-1: per-match triplets + line-text snippets +
 *  total/incomplete in one call). */
type WasmSearchSummary = {
  matches: Uint32Array
  snippets: string[]
  total: number
  incomplete: boolean
  free?: () => void
}

type MaybeSummaryEngine = {
  search_summary?: (
    query: string,
    caseSensitive: boolean,
    isRegex: boolean,
    maxMatches: number
  ) => WasmSearchSummary
}

export type EngineSearchSummaryFn = (
  query: string,
  caseSensitive: boolean,
  isRegex: boolean,
  maxMatches: number
) => EngineFederatedSummary | null

/** Detect `search_summary` on an engine; undefined on older pins. The wrapper
 *  returns null (callers degrade to the triplet shape) if the binding throws or
 *  reports an unexpected shape — artifact skew must never crash the worker. */
export function detectEngineSearchSummary(engine: unknown): EngineSearchSummaryFn | undefined {
  const candidate = engine as MaybeSummaryEngine
  if (typeof candidate.search_summary !== 'function') {
    return undefined
  }
  return (query, caseSensitive, isRegex, maxMatches) => {
    let raw: WasmSearchSummary
    try {
      raw = candidate.search_summary!(query, caseSensitive, isRegex, maxMatches)
    } catch {
      return null
    }
    try {
      const flat = raw.matches
      const snippets = raw.snippets
      if (!(flat instanceof Uint32Array) || !Array.isArray(snippets)) {
        return null
      }
      const matches: AtermFederatedMatch[] = []
      for (let i = 0; i + 3 <= flat.length; i += 3) {
        matches.push({
          absRow: flat[i],
          col: flat[i + 1],
          len: flat[i + 2],
          snippet: typeof snippets[i / 3] === 'string' ? snippets[i / 3] : null
        })
      }
      return {
        matches,
        total: typeof raw.total === 'number' ? raw.total : matches.length,
        incomplete: raw.incomplete === true
      }
    } finally {
      try {
        raw.free?.()
      } catch {
        /* ignore */
      }
    }
  }
}

/** Detect the warm-index release binding (W4A "release-on-close/idle eviction");
 *  undefined on pins without it — §4 eviction then relies on budgeted-cursor
 *  cancel, which frees in-flight (not completed-warm) index state. */
export function detectEngineSearchIndexRelease(engine: unknown): (() => void) | undefined {
  const candidate = engine as { search_index_release?: () => void }
  if (typeof candidate.search_index_release !== 'function') {
    return undefined
  }
  return () => {
    try {
      candidate.search_index_release!()
    } catch {
      /* ignore — eviction is best-effort on skewed artifacts */
    }
  }
}
