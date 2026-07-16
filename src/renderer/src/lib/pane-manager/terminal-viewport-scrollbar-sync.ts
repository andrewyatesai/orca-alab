import type { Terminal } from './aterm/terminal-types'

// Why: a restore can leave the scrollbar thumb stale when the viewport line is
// unchanged. A synchronous one-line jiggle (net zero) resyncs the thumb without
// a visible paint; on the aterm facade both calls route to the engine and the
// draw scheduler coalesces them into one frame.
export function forceTerminalViewportScrollbarSync(terminal: Terminal): void {
  const buf = terminal.buffer.active
  if (buf.viewportY >= buf.baseY) {
    // Why: jiggle-scrolling at bottom makes xterm stop following active output
    // after split-pane resizes; scrollToBottom already places the thumb there.
    return
  }
  if (buf.viewportY > 0) {
    safeScrollCall(() => terminal.scrollLines(-1))
    safeScrollCall(() => terminal.scrollLines(1))
  } else if (buf.viewportY < buf.baseY) {
    safeScrollCall(() => terminal.scrollLines(1))
    safeScrollCall(() => terminal.scrollLines(-1))
  }
}

function safeScrollCall(fn: () => void): void {
  try {
    fn()
  } catch (error) {
    if (!(error instanceof TypeError) || !/dimensions/.test(error.message)) {
      throw error
    }
  }
}
