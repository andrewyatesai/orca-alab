import { loadAterm } from './load-aterm'
import { loadAtermGpu } from './load-aterm-gpu'
import type { AtermThemeColors } from './aterm-theme-colors'
import type { LatencyStats } from './aterm-latency-bench-types'
import { CELL_A, CELL_B, fillBytes, summarize, timeEngineFrames } from './aterm-latency-measure'

// aterm-side benches for the keystroke-latency benchmark: render-half latency
// (single-cell process→render→present, median/p95) and per-frame cost, on both the
// CPU (wasm rasterize + putImageData) and GPU (WebGL2 render + gl.finish) paths.

type SizeOpts = {
  cols: number
  rows: number
  fontPx: number
  themeColors: AtermThemeColors
}

/** Build a CPU engine on a real 2d canvas and time `iterations` single-cell
 *  process→render→blit updates (the render half of one keystroke). */
export async function measureCpuRenderHalf(
  opts: SizeOpts & { iterations: number; warmup: number }
): Promise<LatencyStats> {
  const { cols, rows, fontPx, themeColors, iterations, warmup } = opts
  const { AtermTerminal, fontBytes } = await loadAterm()
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
  const canvas = document.createElement('canvas')
  const ctx = canvas.getContext('2d')
  if (!ctx) {
    term.free()
    throw new Error('cpu latency bench: no 2d context')
  }
  term.process(fillBytes(cols, rows))

  const tick = (i: number): void => {
    term.process(i % 2 === 0 ? CELL_A : CELL_B)
    term.render()
    const w = term.width
    const h = term.height
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w
      canvas.height = h
    }
    const rgba = term.rgba()
    ctx.putImageData(
      new ImageData(
        new Uint8ClampedArray(rgba.buffer as ArrayBuffer, rgba.byteOffset, rgba.byteLength),
        w,
        h
      ),
      0,
      0
    )
  }

  for (let i = 0; i < warmup; i++) {
    tick(i)
  }
  const samples: number[] = []
  for (let i = 0; i < iterations; i++) {
    const t0 = performance.now()
    tick(i)
    samples.push(performance.now() - t0)
  }
  term.free()
  return summarize(samples)
}

/** Build a GPU engine on a webgl2 canvas and time `iterations` single-cell
 *  process→render→gl.finish updates. gl.finish forces real GPU completion so each
 *  timed sample is a finished present, not a queue-and-return. */
export async function measureGpuRenderHalf(
  opts: SizeOpts & { iterations: number; warmup: number }
): Promise<{ stats: LatencyStats; adapterInfo: string | null }> {
  const { cols, rows, fontPx, themeColors, iterations, warmup } = opts
  const { AtermGpuTerminal, fontBytes } = await loadAtermGpu()
  const gpu = new AtermGpuTerminal(
    rows,
    cols,
    fontBytes,
    fontPx,
    themeColors.fg,
    themeColors.bg,
    themeColors.cursor,
    themeColors.selection
  )
  const canvas = document.createElement('canvas')
  await gpu.init(canvas)
  const gl = canvas.getContext('webgl2')
  if (!gl) {
    gpu.free()
    throw new Error('gpu latency bench: no webgl2 context to sync')
  }
  gpu.process(fillBytes(cols, rows))

  const tick = (i: number): void => {
    gpu.process(i % 2 === 0 ? CELL_A : CELL_B)
    gpu.render()
    gl.finish()
  }

  for (let i = 0; i < warmup; i++) {
    tick(i)
  }
  const samples: number[] = []
  for (let i = 0; i < iterations; i++) {
    const t0 = performance.now()
    tick(i)
    samples.push(performance.now() - t0)
  }
  const adapterInfo = gpu.adapter_info || null
  gpu.free()
  return { stats: summarize(samples), adapterInfo }
}

export async function benchAtermCpuFrame(opts: SizeOpts & { frames: number }): Promise<number> {
  const { cols, rows, fontPx, themeColors, frames } = opts
  const { AtermTerminal, fontBytes } = await loadAterm()
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
  const canvas = document.createElement('canvas')
  const ctx = canvas.getContext('2d')
  if (!ctx) {
    term.free()
    throw new Error('cpu frame bench: no 2d context')
  }
  term.process(fillBytes(cols, rows))
  const present = (): void => {
    term.render()
    const w = term.width
    const h = term.height
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w
      canvas.height = h
    }
    const rgba = term.rgba()
    ctx.putImageData(
      new ImageData(
        new Uint8ClampedArray(rgba.buffer as ArrayBuffer, rgba.byteOffset, rgba.byteLength),
        w,
        h
      ),
      0,
      0
    )
  }
  const ms = timeEngineFrames(term, frames, present)
  term.free()
  return ms
}

export async function benchAtermGpuFrame(
  opts: SizeOpts & { frames: number }
): Promise<{ ms: number; adapterInfo: string | null }> {
  const { cols, rows, fontPx, themeColors, frames } = opts
  const { AtermGpuTerminal, fontBytes } = await loadAtermGpu()
  const gpu = new AtermGpuTerminal(
    rows,
    cols,
    fontBytes,
    fontPx,
    themeColors.fg,
    themeColors.bg,
    themeColors.cursor,
    themeColors.selection
  )
  const canvas = document.createElement('canvas')
  await gpu.init(canvas)
  const gl = canvas.getContext('webgl2')
  if (!gl) {
    gpu.free()
    throw new Error('gpu frame bench: no webgl2 context to sync')
  }
  gpu.process(fillBytes(cols, rows))
  const present = (): void => {
    gpu.render()
    gl.finish()
  }
  const ms = timeEngineFrames(gpu, frames, present)
  const adapterInfo = gpu.adapter_info || null
  gpu.free()
  return { ms, adapterInfo }
}
