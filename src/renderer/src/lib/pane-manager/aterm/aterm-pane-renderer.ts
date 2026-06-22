import { loadAterm } from './load-aterm'
import { encodeKeyEventToBytes } from './aterm-key-encoding'
import type { AtermTerminal } from './aterm_wasm.js'

// Font cell size in CSS pixels; multiplied by devicePixelRatio for the engine.
export const ATERM_RENDERER_FONT_PX = 14

export type AtermPaneInputSink = (data: string) => void
export type AtermPaneResizeSink = (cols: number, rows: number) => void

export type AtermPaneController = {
  /** Feed PTY/replay output bytes; coalesces draws into one rAF frame. */
  process: (data: string) => void
  dispose: () => void
}

const MIN_GRID_COLS = 1
const MIN_GRID_ROWS = 1

function computeGrid(
  container: HTMLElement,
  dpr: number,
  cellWidth: number,
  cellHeight: number
): { cols: number; rows: number } {
  const deviceWidth = container.clientWidth * dpr
  const deviceHeight = container.clientHeight * dpr
  const cols = Math.max(MIN_GRID_COLS, Math.floor(deviceWidth / cellWidth))
  const rows = Math.max(MIN_GRID_ROWS, Math.floor(deviceHeight / cellHeight))
  return { cols, rows }
}

export async function createAtermPaneController(
  container: HTMLElement,
  onInput: AtermPaneInputSink,
  onResize: AtermPaneResizeSink
): Promise<AtermPaneController> {
  const canvas = document.createElement('canvas')
  // Fill the pane; pixelated keeps the CPU-rasterized framebuffer crisp when
  // the device-pixel canvas is scaled to CSS pixels.
  canvas.style.width = '100%'
  canvas.style.height = '100%'
  canvas.style.display = 'block'
  canvas.style.imageRendering = 'pixelated'
  canvas.style.outline = 'none'
  canvas.tabIndex = 0
  container.appendChild(canvas)

  const ctx = canvas.getContext('2d')
  const { AtermTerminal: AtermTerminalCtor, fontBytes } = await loadAterm()

  const dpr = window.devicePixelRatio || 1
  // Build once at an arbitrary 1x1 grid to read the engine's cell metrics, then
  // size the real grid to the container.
  const term: AtermTerminal = new AtermTerminalCtor(
    MIN_GRID_ROWS,
    MIN_GRID_COLS,
    fontBytes,
    Math.round(ATERM_RENDERER_FONT_PX * dpr)
  )
  const cellWidth = term.cell_width
  const cellHeight = term.cell_height

  const inputSink = onInput
  const resizeSink = onResize
  let disposed = false
  let drawScheduled = false
  const initialGrid = computeGrid(container, dpr, cellWidth, cellHeight)
  term.resize(initialGrid.rows, initialGrid.cols)
  let cols = initialGrid.cols
  let rows = initialGrid.rows

  const draw = (): void => {
    drawScheduled = false
    if (disposed || !ctx) {
      return
    }
    term.render()
    const width = term.width
    const height = term.height
    canvas.width = width
    canvas.height = height
    // CSS size in logical pixels so the device-pixel framebuffer maps 1:1.
    canvas.style.width = `${width / dpr}px`
    canvas.style.height = `${height / dpr}px`
    ctx.putImageData(new ImageData(new Uint8ClampedArray(term.rgba()), width, height), 0, 0)
  }

  const scheduleDraw = (): void => {
    if (drawScheduled || disposed) {
      return
    }
    drawScheduled = true
    requestAnimationFrame(draw)
  }

  const process = (data: string): void => {
    if (disposed) {
      return
    }
    term.process(new TextEncoder().encode(data))
    scheduleDraw()
  }

  const resizeObserver = new ResizeObserver(() => {
    if (disposed) {
      return
    }
    const next = computeGrid(container, dpr, cellWidth, cellHeight)
    if (next.cols === cols && next.rows === rows) {
      return
    }
    cols = next.cols
    rows = next.rows
    term.resize(rows, cols)
    // Mirror the new grid to the PTY so the child re-wraps for the new size.
    resizeSink(cols, rows)
    scheduleDraw()
  })
  resizeObserver.observe(container)

  const onKeyDown = (event: KeyboardEvent): void => {
    const bytes = encodeKeyEventToBytes(event)
    if (bytes === null) {
      return
    }
    event.preventDefault()
    inputSink(bytes)
  }
  canvas.addEventListener('keydown', onKeyDown)

  const onPointerDown = (): void => {
    canvas.focus()
  }
  canvas.addEventListener('pointerdown', onPointerDown)

  // Report the initial grid so the PTY spawns/resizes to match the canvas.
  resizeSink(cols, rows)
  scheduleDraw()

  return {
    process,
    dispose: () => {
      if (disposed) {
        return
      }
      disposed = true
      resizeObserver.disconnect()
      canvas.removeEventListener('keydown', onKeyDown)
      canvas.removeEventListener('pointerdown', onPointerDown)
      canvas.remove()
      try {
        term.free()
      } catch {
        /* ignore */
      }
    }
  }
}
