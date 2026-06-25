import type { AtermTerminal } from './aterm_wasm'

/** OSC 0/2 window-title channel re-homed off the shadow xterm: track the engine's
 *  title and notify subscribers (agent detection / mobile streaming) on change, so
 *  the title source no longer depends on xterm's onTitleChange. */
export function createAtermTitleChannel(term: Pick<AtermTerminal, 'title'>): {
  /** Re-read the engine title and fire listeners if it changed (call post-process). */
  emitIfChanged: () => void
  /** Current title, or null when unset. */
  title: () => string | null
  /** Subscribe to title changes; returns an xterm-compatible disposable. */
  onTitleChange: (handler: (title: string) => void) => { dispose: () => void }
} {
  let lastTitle = term.title() ?? ''
  const listeners = new Set<(title: string) => void>()
  return {
    emitIfChanged: () => {
      const next = term.title() ?? ''
      if (next !== lastTitle) {
        lastTitle = next
        listeners.forEach((listener) => listener(next))
      }
    },
    title: () => term.title() ?? null,
    onTitleChange: (handler) => {
      listeners.add(handler)
      return { dispose: () => void listeners.delete(handler) }
    }
  }
}
