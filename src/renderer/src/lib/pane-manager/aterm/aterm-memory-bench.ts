import { loadAterm } from './load-aterm'
import type { AtermThemeColors } from './aterm-theme-colors'

// e2e-only PER-PANE MEMORY benchmark — answers the adversarial review's "you load a
// whole VT engine per pane, how much does that cost?" with a real number. It builds
// several LIVE aterm engines (no free between them) each fed N lines of scrollback +
// rendered, and divides the wasm linear-memory growth by the pane count. Measuring
// live engines (not a construct/free delta) avoids the allocator reusing a freed
// arena, which would understate the cost. Fonts are NOT injected here: the OS
// fallback fonts (the big bytes) are interned to ONE shared copy across all panes
// (aterm-render intern + its unit test), so they're a one-time cost, not per-pane.

export type AtermMemoryBenchResult = {
  panes: number
  scrollbackLines: number
  cols: number
  rows: number
  /** wasm heap growth attributable to one pane (grid + scrollback + CPU framebuffer
   *  + glyph atlas), in bytes — the per-pane variable cost. */
  bytesPerPane: number
  kbPerPane: number
  /** Total wasm linear memory after building all panes (process-wide, shared). */
  totalHeapBytes: number
}

export async function benchAtermMemory(opts: {
  cols: number
  rows: number
  scrollbackLines: number
  panes: number
  fontPx: number
  themeColors: AtermThemeColors
}): Promise<AtermMemoryBenchResult> {
  const { cols, rows, scrollbackLines, panes, fontPx, themeColors } = opts
  const { AtermTerminal, fontBytes, memory } = await loadAterm()
  const enc = new TextEncoder()

  const build = (): InstanceType<typeof AtermTerminal> => {
    const term = new AtermTerminal(
      rows,
      cols,
      fontBytes,
      fontPx,
      themeColors.fg,
      themeColors.bg,
      themeColors.cursor,
      themeColors.selection
    )
    for (let i = 0; i < scrollbackLines; i++) {
      term.process(enc.encode(`line ${i} ${'x'.repeat(Math.max(0, cols - 10))}\r\n`))
    }
    term.render()
    return term
  }

  // Warm one engine (first-time wasm arena growth / lazy statics) so it isn't
  // attributed to the measured panes; free it before measuring.
  const warm = build()
  warm.free()

  const before = memory.buffer.byteLength
  const live: InstanceType<typeof AtermTerminal>[] = []
  for (let k = 0; k < panes; k++) {
    live.push(build())
  }
  const after = memory.buffer.byteLength
  const bytesPerPane = Math.max(0, Math.round((after - before) / panes))
  for (const term of live) {
    term.free()
  }

  return {
    panes,
    scrollbackLines,
    cols,
    rows,
    bytesPerPane,
    kbPerPane: Math.round(bytesPerPane / 1024),
    totalHeapBytes: after
  }
}
