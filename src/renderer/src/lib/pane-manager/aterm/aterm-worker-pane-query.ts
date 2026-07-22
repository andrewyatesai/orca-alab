// Answers a pane-scoped 'query' (the id-correlated channel): cold engine reads, the
// parse fence, and searchFind with its supersede skip. Split from the pane dispatch
// to keep that file under the line cap.

import type { PaneRuntime } from './aterm-worker-pane-dispatch'
import type { AtermWorkerQuery } from './aterm-render-worker-protocol'

/** Answer one query for `pane`, posting its id-correlated 'queryResult'. */
export function answerPaneQuery(pane: PaneRuntime, msg: AtermWorkerQuery): void {
  const term = pane.term
  if (msg.kind === 'searchFind') {
    // Superseded find (a newer one already ARRIVED — this one sat behind a flood
    // backlog): answer null without paying the engine search. Its promise was
    // already cancelled on the main thread; the null reply is a harmless no-op.
    if (msg.id < pane.latestSearchFindQueryId) {
      pane.post({ type: 'queryResult', id: msg.id, value: null })
      return
    }
    if (!term) {
      pane.post({ type: 'queryResult', id: msg.id, value: null })
      return
    }
    // msg.id doubles as the request generation the STATE echoes (searchGeneration).
    // The find runs in budget-sliced engine calls that yield to this message loop;
    // the reply posts when it completes (sync for small buffers) or as null when a
    // newer find cancelled it mid-slices.
    term.searchFindQuery(
      msg.arg,
      msg.text,
      msg.id,
      // Polled between slices: the entry records the newest find id at ARRIVAL, so
      // a newer find still queued behind the scheduler already cancels this one.
      () => msg.id < pane.latestSearchFindQueryId,
      (value) => {
        if (value !== null) {
          // A completed find scrolls the active match into view + changes the
          // highlight rects — repaint + STATE, exactly like the search nav commands.
          pane.frameScheduler.schedule()
        }
        pane.post({ type: 'queryResult', id: msg.id, value })
      }
    )
    return
  }
  // 'flush' is a parse fence, not an engine read: reaching it means every earlier
  // message (process bytes + their posted replies) was handled, so answer directly —
  // even with no engine yet.
  const value = msg.kind === 'flush' ? true : term ? term.query(msg.kind, msg.arg, msg.arg2) : null
  pane.post({ type: 'queryResult', id: msg.id, value })
}
