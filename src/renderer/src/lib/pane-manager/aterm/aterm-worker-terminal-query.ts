// Answers the engine-side reads of a pane 'query' (the id-correlated channel):
// serialize / cold content reads / link hits, plus searchFind (never cost-gated;
// the query id doubles as the request generation echoed via STATE.searchGeneration).
// Split from the worker terminal to keep that file under the line cap.

import { answerSearchFindQuery, type WorkerSearch } from './aterm-worker-search'
import type { EngineHandle } from './aterm-worker-engine-build'

export function answerWorkerTerminalQuery(
  e: EngineHandle['engine'],
  search: WorkerSearch,
  kind: string,
  arg: number | undefined,
  arg2: number | undefined,
  text?: string,
  queryId?: number
): string | number | boolean | null {
  switch (kind) {
    case 'searchFind':
      return answerSearchFindQuery(search, arg, text, queryId ?? 0)
    case 'serialize':
      return e.serialize(arg ?? undefined)
    case 'serializeScrollback':
      return e.serialize_scrollback(arg ?? undefined)
    case 'selectionText':
      return e.selection_text() ?? ''
    case 'rowText':
      return e.row_text(arg ?? 0) ?? null
    case 'rowLen':
      return e.row_len(arg ?? 0) ?? null
    case 'rowWrapped':
      return e.row_is_wrapped(arg ?? 0) ?? null
    case 'cellText':
      return e.cell_text(arg ?? 0, arg2 ?? 0)
    case 'cellWide':
      return e.cell_is_wide(arg ?? 0, arg2 ?? 0) ?? null
    case 'linkAt': {
      const hit = e.link_at(arg ?? 0, arg2 ?? 0)
      return hit
        ? JSON.stringify({
            url: hit.url,
            kind: hit.kind,
            start_col: hit.start_col,
            end_col: hit.end_col
          })
        : null
    }
    default:
      return null
  }
}
