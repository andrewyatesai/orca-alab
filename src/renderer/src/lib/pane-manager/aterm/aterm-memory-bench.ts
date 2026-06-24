import { loadAterm } from './load-aterm'
import type { AtermThemeColors } from './aterm-theme-colors'

// e2e-only PER-PANE MEMORY benchmark — answers the adversarial review's "you load a
// whole VT engine per pane, how much does that cost?" with a real number. It builds
// several LIVE aterm engines (no free between them) each fed N lines of scrollback +
// rendered, and divides the wasm linear-memory growth by the pane count. Measuring
// live engines (not a construct/free delta) avoids the allocator reusing a freed
// arena, which would understate the cost.
//
// Two honesty constraints this encodes:
//   * It builds the CPU-fallback engine (aterm-wasm), whose wasm heap holds the
//     RGBA framebuffer — so the number is the wasm-footprint UPPER BOUND. The
//     shipped GPU default (aterm-gpu-web) keeps that framebuffer in GPU textures,
//     not the wasm heap, so it is LIGHTER per pane than this figure.
//   * Content is GLYPH-DIVERSE (printable ASCII + box-drawing + block + Latin-1),
//     not a single repeated char, so the per-pane glyph atlas is populated like a
//     real working pane's — a single-glyph atlas would under-report it.
// The big OS fallback fonts (CJK + colour emoji) are NOT injected here: they are
// interned to ONE shared copy across all panes (aterm-render intern + its unit
// test), so they're a one-time cost, not per-pane.

// A representative spread of the glyphs a working terminal actually rasterizes:
// full printable ASCII, box-drawing + block elements (TUI frames/bars), and a few
// Latin-1 accented letters. Rotating windows of this across the scrollback lines
// populate the per-pane atlas with the real glyph set (vs a single 'x').
const GLYPH_POOL =
  ' !"#$%&\'()*+,-./0123456789:;<=>?@' +
  'ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`' +
  'abcdefghijklmnopqrstuvwxyz{|}~' +
  '─│┌┐└┘├┤┬┴┼═║╔╗╚╝╠╣╦╩╬' +
  '█▀▄▌▐░▒▓■□▲▼◆●○' +
  'áéíóúñàèçßäöü°±×÷©®™'

/** A full-width line of length `cols`, rotating its start through GLYPH_POOL so a
 *  handful of lines rasterize the whole spread (then cache-hit, as a real pane does). */
function glyphDiverseLine(index: number, cols: number): string {
  const start = (index * 7) % GLYPH_POOL.length
  let s = ''
  while (s.length < cols) {
    s += GLYPH_POOL.slice(start)
  }
  return s.slice(0, cols)
}

export type AtermMemoryBenchResult = {
  panes: number
  scrollbackLines: number
  cols: number
  rows: number
  /** wasm heap growth attributable to one pane (grid + scrollback + CPU framebuffer
   *  + glyph atlas), in bytes — the per-pane variable cost, and the UPPER BOUND:
   *  the GPU default keeps the framebuffer in GPU textures, not the wasm heap. */
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
      term.process(enc.encode(`${glyphDiverseLine(i, Math.max(0, cols - 1))}\r\n`))
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
