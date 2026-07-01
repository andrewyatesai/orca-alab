// TS dispatch for the quick-open-rank parity module: maps the shared vector
// function names to the real quick-open ranking exports so the harness compares
// the live TS reference against the Rust port. The vector passes RAW paths; this
// runs prepareQuickOpenFiles + rankQuickOpenFiles, exactly as the renderer does.

import {
  prepareQuickOpenFiles,
  QUICK_OPEN_RESULT_LIMIT,
  rankQuickOpenFiles
} from '../../../src/renderer/src/components/quick-open-search'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'rankQuickOpenFiles': {
      const { query, paths, limit } = input as {
        query: string
        paths: string[]
        limit?: number
      }
      const indexed = prepareQuickOpenFiles(paths)
      const results = rankQuickOpenFiles(query, indexed, limit ?? QUICK_OPEN_RESULT_LIMIT)
      return results.map((result) => ({ path: result.path, score: result.score }))
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
