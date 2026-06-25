import { loadAterm } from './load-aterm'
import { loadAtermGpu } from './load-aterm-gpu'
import type { AtermThemeColors } from './aterm-theme-colors'
import type {
  AtermLatencyBenchResult,
  AtermLatencyRenderHalf,
  FrameTimeRow,
  LatencyStats
} from './aterm-latency-bench-types'
import { probeGl } from './aterm-latency-measure'
import {
  benchAtermCpuFrame,
  benchAtermGpuFrame,
  measureCpuRenderHalf,
  measureGpuRenderHalf
} from './aterm-latency-aterm-bench'

export type {
  AtermLatencyBenchResult,
  AtermLatencyRenderHalf,
  FrameTimeRow,
  LatencyStats
} from './aterm-latency-bench-types'

/** e2e-only KEYSTROKE-LATENCY benchmark for the aterm renderer. Runs the RENDER
 *  HALF of that latency honestly:
 *
 *   - aterm CPU: process(one cell) → render() → putImageData blit, exactly what the
 *     live CPU pane's draw scheduler runs per output chunk.
 *   - aterm GPU: process(one cell) → render() → gl.finish(), the full WebGL2 present
 *     forced to GPU completion (render() alone only queues commands).
 *
 *  N iterations → median + p95 (latency is about the tail). The render-half is the
 *  render contribution to one keystroke; the shared PTY round-trip is excluded. */
export async function benchAtermLatency(opts: {
  sizes: [number, number][]
  iterations: number
  warmup: number
  frames: number
  fontPx: number
  themeColors: AtermThemeColors
}): Promise<AtermLatencyBenchResult> {
  const { sizes, iterations, warmup, frames, fontPx, themeColors } = opts
  const gl = probeGl()

  // Warm the wasm + font loaders once before any timing (loaders memoize).
  await Promise.all([loadAterm(), loadAtermGpu()])

  let gpuAdapterInfo: string | null = null

  // Render-half latency at the typical 80x24 grid (median + p95 of single-cell
  // process→render→present).
  const cpu = await measureCpuRenderHalf({
    cols: 80,
    rows: 24,
    fontPx,
    themeColors,
    iterations,
    warmup
  })
  let gpuHalf: LatencyStats | null = null
  let gpuReason: string | undefined
  try {
    const g = await measureGpuRenderHalf({
      cols: 80,
      rows: 24,
      fontPx,
      themeColors,
      iterations,
      warmup
    })
    gpuHalf = g.stats
    gpuAdapterInfo ??= g.adapterInfo
  } catch (err) {
    gpuReason = String(err)
  }

  const renderHalf: AtermLatencyRenderHalf = { cpu, gpu: gpuHalf, gpuReason }

  // Head-to-head per-frame table at each requested grid.
  const frameTable: FrameTimeRow[] = []
  for (const [cols, rows] of sizes) {
    const atermCpuMsPerFrame = await benchAtermCpuFrame({ cols, rows, fontPx, themeColors, frames })

    let atermGpuMsPerFrame: number | null = null
    try {
      const g = await benchAtermGpuFrame({ cols, rows, fontPx, themeColors, frames })
      atermGpuMsPerFrame = g.ms
      gpuAdapterInfo ??= g.adapterInfo
    } catch {
      atermGpuMsPerFrame = null
    }

    frameTable.push({
      cols,
      rows,
      atermCpuMsPerFrame,
      atermGpuMsPerFrame
    })
  }

  return {
    glRenderer: gl.renderer,
    glVendor: gl.vendor,
    gpuAdapterInfo,
    renderHalf,
    frameTable
  }
}
