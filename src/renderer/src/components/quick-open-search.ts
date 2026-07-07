import { isClipboardTextByteLengthOverLimit } from '../../../shared/clipboard-text'

// The fuzzy ranking (prepare + rank) moved to the Rust orca-text core in the
// orca-git wasm: src/renderer/src/lib/git-wasm/quick-open.ts holds a prepared
// index in wasm linear memory, so only the query crosses per keystroke
// (~4x faster than the deleted TS scan, identical results). This module keeps
// only the query-size guard, which rides the live shared clipboard-text limit.
export { QUICK_OPEN_RESULT_LIMIT } from '../lib/git-wasm/quick-open'
export type { QuickOpenSearchResult } from '../lib/git-wasm/quick-open'

export const QUICK_OPEN_QUERY_MAX_BYTES = 2 * 1024

export function isQuickOpenQueryTooLarge(
  query: string,
  maxBytes = QUICK_OPEN_QUERY_MAX_BYTES
): boolean {
  return isClipboardTextByteLengthOverLimit(query, maxBytes)
}
