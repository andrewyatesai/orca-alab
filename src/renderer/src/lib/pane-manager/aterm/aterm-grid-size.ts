/** Grid-size math for the aterm canvas: turn a container's CSS size + device
 *  pixel ratio + the engine's cell metrics into a (cols, rows) grid. Factored
 *  out of the controller so it stays under the line budget. */

export const MIN_GRID_COLS = 1
export const MIN_GRID_ROWS = 1
const DEFAULT_GRID_COLS = 80
const DEFAULT_GRID_ROWS = 24

/** Compute the (cols, rows) the canvas should render for `container`. Falls back
 *  to a standard 80x24 when the container isn't laid out yet (hidden/background
 *  pane, pre-mount) so the terminal is usable; the ResizeObserver corrects it
 *  once the pane has real dimensions. Never returns a 1x1 grid for a laid-out
 *  container. `cellWidth`/`cellHeight` are device-pixel cell metrics. */
export function computeGrid(
  container: HTMLElement,
  dpr: number,
  cellWidth: number,
  cellHeight: number
): { cols: number; rows: number } {
  const deviceWidth = container.clientWidth * dpr
  const deviceHeight = container.clientHeight * dpr
  if (deviceWidth < cellWidth || deviceHeight < cellHeight) {
    return { cols: DEFAULT_GRID_COLS, rows: DEFAULT_GRID_ROWS }
  }
  const cols = Math.max(MIN_GRID_COLS, Math.floor(deviceWidth / cellWidth))
  const rows = Math.max(MIN_GRID_ROWS, Math.floor(deviceHeight / cellHeight))
  return { cols, rows }
}
