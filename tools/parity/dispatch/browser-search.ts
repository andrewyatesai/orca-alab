// TS dispatch for the browser-search parity module: maps the shared vector
// function names to the real `src/shared/browser-url.ts` exports so the harness
// compares the live TS reference against the Rust port. Only the self-contained
// pure functions are covered; the Kagi session-link / navigation normaliser is
// deferred in the Rust port.

import { buildSearchUrl, looksLikeSearchQuery, type SearchEngine } from '../../../src/shared/browser-url'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'buildSearchUrl': {
      const { query, engine } = input as { query: string; engine?: SearchEngine }
      // Pass engine through; undefined triggers the `engine = DEFAULT_SEARCH_ENGINE` default param.
      return buildSearchUrl(query, engine)
    }
    case 'looksLikeSearchQuery':
      return looksLikeSearchQuery(input as string)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
