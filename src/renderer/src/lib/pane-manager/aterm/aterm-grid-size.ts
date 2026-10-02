/** Grid-size math for the aterm canvas: turn a container's CSS size + device
 *  pixel ratio + the engine's cell metrics into a (cols, rows) grid. Factored
 *  out of the controller so it stays under the line budget. */

import { TERMINAL_GRID_MAX_DIM } from '../../../../../shared/terminal-grid-limits'

export const MIN_GRID_COLS = 1
export const MIN_GRID_ROWS = 1
/** The daemon refuses a grid above this per dimension (terminal-grid-limits.ts). */
export const MAX_GRID_COLS = TERMINAL_GRID_MAX_DIM
export const MAX_GRID_ROWS = TERMINAL_GRID_MAX_DIM
const DEFAULT_GRID_COLS = 80
const DEFAULT_GRID_ROWS = 24

/** Compute the (cols, rows) the canvas should render for `container`. Falls back
 *  to a standard 80x24 when the container isn't laid out yet (hidden/background
 *  pane, pre-mount) so the terminal is usable; the ResizeObserver corrects it
 *  once the pane has real dimensions. Never returns a 1x1 grid for a laid-out
 *  container. `cellWidth`/`cellHeight` are device-pixel cell metrics.
 *  `measured` distinguishes a real layout-derived grid from that fallback —
 *  fallback dims must never be reported to a live PTY (the placeholder kernel-
 *  SIGWINCHes TUIs into a bogus relayout before the observer corrects it). */
export function computeGrid(
  container: HTMLElement,
  dpr: number,
  cellWidth: number,
  cellHeight: number
): { cols: number; rows: number; measured: boolean } {
  const deviceWidth = container.clientWidth * dpr
  const deviceHeight = container.clientHeight * dpr
  if (deviceWidth < cellWidth || deviceHeight < cellHeight) {
    return { cols: DEFAULT_GRID_COLS, rows: DEFAULT_GRID_ROWS, measured: false }
  }
  // Why the upper clamp: the daemon refuses a create/resize above 4096 per
  // dimension; a canvas wider than 4096 cells just leaves unused pixels.
  const cols = Math.min(MAX_GRID_COLS, Math.max(MIN_GRID_COLS, Math.floor(deviceWidth / cellWidth)))
  const rows = Math.min(
    MAX_GRID_ROWS,
    Math.max(MIN_GRID_ROWS, Math.floor(deviceHeight / cellHeight))
  )
  return { cols, rows, measured: true }
}
