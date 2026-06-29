import type { AtermTerminal } from './aterm_wasm'
import { drainAtermReplies } from './aterm-reply-drain'

type ProcessPumpDeps = {
  term: Pick<AtermTerminal, 'process_str' | 'display_offset' | 'scroll_to_bottom' | 'take_response'>
  inputSink: (data: string) => void
  isDisposed: () => boolean
  /** Re-read the engine title + fire title listeners (post-process). */
  emitTitleIfChanged: () => void
  /** True if a search query is active, so highlights need re-running this frame. */
  hasActiveSearchQuery: () => boolean
  /** Mark that the next draw must refresh search highlights. */
  markSearchRefresh: () => void
  scheduleDraw: () => void
}

/** Build the PTY/replay byte pump: feed the engine, drain its authoritative query
 *  replies + title, follow the live bottom (only if already there), flag a search
 *  refresh, and schedule a draw. Extracted to keep the wiring file focused. */
export function createAtermProcessPump(deps: ProcessPumpDeps): (data: string) => void {
  return (data: string): void => {
    if (deps.isDisposed()) {
      return
    }
    // Follow the bottom on new output ONLY if already at the bottom (aterm SCR-1).
    const wasAtBottom = deps.term.display_offset === 0
    // process_str hands the string straight to wasm-bindgen (encodeInto into wasm
    // memory) — no JS-side TextEncoder alloc + copy per chunk on the hot path.
    deps.term.process_str(data)
    // aterm is the authoritative query responder — drain + forward its replies.
    drainAtermReplies(deps.term, deps.inputSink)
    deps.emitTitleIfChanged()
    if (wasAtBottom && deps.term.display_offset !== 0) {
      deps.term.scroll_to_bottom()
    }
    if (deps.hasActiveSearchQuery()) {
      deps.markSearchRefresh()
    }
    deps.scheduleDraw()
  }
}
