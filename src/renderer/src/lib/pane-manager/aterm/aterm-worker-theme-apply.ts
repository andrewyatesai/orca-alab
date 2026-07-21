import type { WorkerEngine } from './aterm-worker-engine-build'
import type { AtermWorkerThemeSet } from './aterm-render-worker-protocol'

/** Apply one granular theme / reply-default op to the worker engine — applyAtermLiveTheme
 *  fans out to these on a live re-theme (palette + OSC 10/11 + CSI 14t/16t reply state) so
 *  the engine restyles without a pane rebuild. Extracted from the worker terminal for its
 *  line budget, alongside its search / dirty-row / effects splits. */
export function applyAtermWorkerThemeSet(e: WorkerEngine, m: AtermWorkerThemeSet): void {
  switch (m.op) {
    case 'theme':
      e.set_theme(m.fg, m.bg, m.cursor, m.selection)
      return
    case 'paletteColor':
      e.set_palette_color(m.index, m.r, m.g, m.b)
      return
    case 'defaultForeground':
      e.set_default_foreground(m.r, m.g, m.b)
      return
    case 'defaultBackground':
      e.set_default_background(m.r, m.g, m.b)
      return
    case 'selectionFg':
      e.set_selection_fg(m.fg ?? undefined)
      return
    case 'cellPixelSize':
      e.set_cell_pixel_size(m.width, m.height)
  }
}
