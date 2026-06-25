// Result shapes for the e2e keystroke-latency benchmark (see aterm-latency-bench.ts).

export type LatencyStats = {
  /** Number of timed samples. */
  samples: number
  medianMs: number
  p95Ms: number
  minMs: number
  maxMs: number
  meanMs: number
}

export type AtermLatencyRenderHalf = {
  /** aterm CPU: process(1 cell) + render() + putImageData. */
  cpu: LatencyStats
  /** aterm GPU: process(1 cell) + render() + gl.finish() (null if GPU failed). */
  gpu: LatencyStats | null
  gpuReason?: string
}

export type FrameTimeRow = {
  cols: number
  rows: number
  /** aterm CPU per-frame ms (render + blit). */
  atermCpuMsPerFrame: number
  /** aterm GPU per-frame ms (render + gl.finish), null if GPU failed. */
  atermGpuMsPerFrame: number | null
}

export type AtermLatencyBenchResult = {
  glRenderer: string | null
  glVendor: string | null
  gpuAdapterInfo: string | null
  /** One-cell-update render-half latency at a typical 80x24 grid. */
  renderHalf: AtermLatencyRenderHalf
  /** Per-frame aterm CPU/GPU render cost at each grid. */
  frameTable: FrameTimeRow[]
}
