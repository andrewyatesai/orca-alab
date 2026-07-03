import type { AtermTerminal } from './aterm_wasm'
import { drainAtermReplies } from './aterm-reply-drain'

// Reused for the GPU engine's byte feed (it has no process_str). One shared encoder
// avoids a per-chunk allocation on the (less common) GPU in-process path.
const processPumpEncoder = new TextEncoder()

type ProcessPumpDeps = {
  // The CPU engine (AtermTerminal) + the worker-backed term expose process_str
  // (encodeInto into wasm, no JS alloc on the hot path); the GPU engine
  // (AtermGpuTerminal) only has process(bytes). Accept either so the in-process GPU
  // path works too.
  term: Pick<AtermTerminal, 'process' | 'display_offset' | 'scroll_to_bottom' | 'take_response'> & {
    process_str?: (s: string) => void
  }
  inputSink: (data: string) => void
  isDisposed: () => boolean
  /** Re-read the engine title + fire title listeners (post-process). */
  emitTitleIfChanged: () => void
  /** Follow a live OSC 12 cursor-colour change into the effects colour
   *  (cheap getter compare; no-op when the colour is unchanged). */
  syncCursorColor?: () => void
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
    // memory) — no JS-side alloc + copy per chunk on the hot path. The GPU engine has
    // no process_str, so fall back to process(encode) there.
    if (deps.term.process_str) {
      deps.term.process_str(data)
    } else {
      deps.term.process(processPumpEncoder.encode(data))
    }
    // aterm is the authoritative query responder — drain + forward its replies.
    drainAtermReplies(deps.term, deps.inputSink)
    deps.emitTitleIfChanged()
    deps.syncCursorColor?.()
    if (wasAtBottom && deps.term.display_offset !== 0) {
      deps.term.scroll_to_bottom()
    }
    if (deps.hasActiveSearchQuery()) {
      deps.markSearchRefresh()
    }
    deps.scheduleDraw()
  }
}
