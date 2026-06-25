import { loadAtermCpuDrawer } from '@/lib/pane-manager/aterm/aterm-cpu-drawer'
import {
  applyAtermLiveTheme,
  type AtermThemeColors
} from '@/lib/pane-manager/aterm/aterm-theme-colors'

// Clear screen + scrollback + home cursor, so a re-feed starts from a clean grid.
const CLEAR_AND_HOME = '\x1b[2J\x1b[3J\x1b[H'

/** Settings-preview terminal driven by the REAL aterm engine (the same CPU
 *  drawer the live pane uses). It loads the wasm engine, sizes a fixed grid,
 *  processes the static preview buffer, and blits the engine's RGBA framebuffer
 *  onto a 2d canvas — no xterm, no fake/screenshot. Theme/font-size changes
 *  re-theme/re-rasterize the same engine in place and repaint. */
export type TerminalPreviewAtermEngine = {
  /** Re-theme the engine in place and repaint (same content). */
  applyTheme(themeColors: AtermThemeColors): void
  /** Change the font px (logical) and re-feed `buffer` (carries the cursor-style
   *  DECSCUSR), re-rasterizing the fixed grid, then repaint. */
  applyFontAndBuffer(fontPx: number, buffer: string): void
  dispose(): void
}

type CreateArgs = {
  canvas: HTMLCanvasElement
  cols: number
  rows: number
  fontPx: number
  themeColors: AtermThemeColors
  /** The preview bytes to process (e.g. PREVIEW_BUFFER + a DECSCUSR cursor set). */
  buffer: string
}

/** Spin up the preview engine. Async because the wasm + fonts load lazily (the
 *  same memoized loaders the live pane uses). Resolves to null when the caller
 *  disposed before load finished (the canvas may be unmounted). */
export async function createTerminalPreviewAtermEngine(
  args: CreateArgs,
  isCancelled: () => boolean
): Promise<TerminalPreviewAtermEngine | null> {
  const ctx = args.canvas.getContext('2d')
  const dpr = window.devicePixelRatio || 1
  // loadAtermCpuDrawer builds the real engine: injects fallback fonts, seeds the
  // theme palette + reply defaults. We drive render()/rgba() directly (a static
  // preview needs none of the pane's scheduler/search/link overlays).
  const pending = await loadAtermCpuDrawer({
    canvas: args.canvas,
    themeColors: args.themeColors,
    fontPx: Math.round(args.fontPx * dpr)
  })
  if (isCancelled()) {
    pending.term.free()
    return null
  }
  const term = pending.term
  const encoder = new TextEncoder()

  const repaint = (): void => {
    if (!ctx) {
      return
    }
    term.render()
    const width = term.width
    const height = term.height
    if (args.canvas.width !== width || args.canvas.height !== height) {
      args.canvas.width = width
      args.canvas.height = height
    }
    const liveDpr = window.devicePixelRatio || 1
    // CSS size = device px / dpr so the framebuffer maps 1:1 (matches the pane).
    args.canvas.style.width = `${width / liveDpr}px`
    args.canvas.style.height = `${height / liveDpr}px`
    ctx.putImageData(new ImageData(new Uint8ClampedArray(term.rgba()), width, height), 0, 0)
  }

  const feedBuffer = (buffer: string, prefix = ''): void => {
    term.resize(args.rows, args.cols)
    term.process(encoder.encode(prefix + buffer))
  }

  feedBuffer(args.buffer)
  repaint()

  return {
    applyTheme(themeColors) {
      // Re-theme the live engine in place (fg/bg/cursor/selection + ANSI palette +
      // reply defaults), then repaint the unchanged grid.
      applyAtermLiveTheme(term, themeColors, term.cell_width, term.cell_height)
      repaint()
    },
    applyFontAndBuffer(fontPx, buffer) {
      const nextDpr = window.devicePixelRatio || 1
      // Re-rasterize the glyph atlas at the new px; cell metrics change, so re-feed
      // the fixed grid from a cleared screen and repaint.
      term.set_px(Math.round(fontPx * nextDpr))
      feedBuffer(buffer, CLEAR_AND_HOME)
      repaint()
    },
    dispose() {
      try {
        term.free()
      } catch {
        /* ignore — engine may already be freed */
      }
    }
  }
}
