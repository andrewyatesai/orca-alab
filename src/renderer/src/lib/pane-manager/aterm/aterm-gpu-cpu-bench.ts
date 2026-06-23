import { loadAterm } from './load-aterm'
import { loadAtermGpu } from './load-aterm-gpu'
import type { AtermThemeColors } from './aterm-theme-colors'

/** e2e-only GPU-vs-CPU FRAME-TIME benchmark. Builds a fresh engine per path at the
 *  SAME grid + theme + font px, fills every cell with dense SGR-colored content,
 *  then times N per-frame draws — toggling a cell each frame so neither engine can
 *  no-op an unchanged grid (matches the existing CPU render benchmark's driver).
 *
 *  Fairness: the CPU per-frame cost is `term.render()` (wasm rasterize) PLUS the
 *  `putImageData` blit onto a real 2d canvas — the actual cost the live CPU pane
 *  pays each frame. The GPU per-frame cost is `gpuTerm.render()`, which already
 *  includes the full present (atlas upload + instanced draw + blit into the WebGL2
 *  swapchain). So both numbers are the real end-to-end per-frame draw for each path.
 *
 *  The first GPU frame (glyph-atlas build + first pipeline submit) is a real
 *  one-time cost, so it is timed and reported SEPARATELY from the steady-state
 *  ms/frame rather than folded into the average. The wasm load/init time (also
 *  one-time, shared across all panes) is captured by the caller. */
export type AtermPathBenchResult = {
  path: 'cpu' | 'gpu'
  mutation: AtermBenchMutation
  cols: number
  rows: number
  frames: number
  width: number
  height: number
  totalMs: number
  msPerFrame: number
  fps: number
  /** First-frame cost: the GPU glyph-atlas build / first submit (or the CPU
   *  first render). Reported apart from the steady-state average. */
  firstFrameMs: number
  /** One-time engine init: the GPU `init(canvas)` acquire+configure (0 for CPU). */
  initMs: number
  /** GPU only: per-frame `render()` WITHOUT the gl.finish() GPU sync — i.e. the
   *  command-submission cost alone. The gap between this and `msPerFrame` is the
   *  GPU completion time the finish() forces; surfaced so a near-zero submit time
   *  can't masquerade as a (sync-free, dishonest) "frame time". 0 for CPU. */
  submitMsPerFrame: number
}

export type AtermGpuCpuBenchResult = {
  available: boolean
  reason?: string
  /** Per-grid CPU + GPU frame-time pairs. */
  rows: AtermBenchRow[]
  /** The WebGL adapter/backend wgpu acquired, for interpreting the GPU numbers. */
  adapterInfo: string | null
  /** UNMASKED_RENDERER_WEBGL of the throwaway probe context (ANGLE/Metal vs sw). */
  glRenderer: string | null
  /** UNMASKED_VENDOR_WEBGL. */
  glVendor: string | null
  /** One-time wasm module load+instantiate cost for each engine (shared). */
  cpuWasmLoadMs: number
  gpuWasmLoadMs: number
}

/** One grid's results for ONE mutation mode: the CPU pair + the GPU pair (null if
 *  the GPU path failed to run that size). */
export type AtermBenchModeRow = {
  cols: number
  rows: number
  mutation: AtermBenchMutation
  cpu: AtermPathBenchResult
  gpu: AtermPathBenchResult | null
}

/** All measurements for one grid: the typical (sparse, 1-cell change) case AND
 *  the worst (full-grid refill) case, so the report shows both. */
export type AtermBenchRow = {
  cols: number
  rows: number
  sparse: AtermBenchModeRow
  full: AtermBenchModeRow
}

/** Dense, per-cell-varied SGR content: every cell gets a printable glyph and the
 *  row carries an SGR color, so the rasterizer does representative work (not a
 *  blank frame). Mirrors the existing CPU render benchmark's fill. */
function fillBytes(cols: number, rows: number): Uint8Array {
  const enc = new TextEncoder()
  const line = (row: number): string => {
    let s = `\x1b[${(row % 7) + 31}m`
    for (let c = 0; c < cols; c++) {
      s += String.fromCharCode(33 + ((row * 7 + c) % 94))
    }
    return `${s}\x1b[0m`
  }
  const body = Array.from({ length: rows }, (_, r) => line(r)).join('\r\n')
  return enc.encode(`\x1b[H${body}`)
}

/** SPARSE per-frame mutation: write one alternating cell at the top-left so the
 *  grid changes every frame (the engine can't short-circuit an unchanged frame).
 *  This is the typical terminal case (most frames touch few cells) and matches the
 *  existing CPU render benchmark's driver; the GPU path's scissored dirty-row
 *  repaint then re-encodes only the changed row, while the CPU always rasterizes
 *  the whole grid — a real architectural difference, so we ALSO measure `full`. */
const MUT_A = new TextEncoder().encode('\x1b[1;1HA')
const MUT_B = new TextEncoder().encode('\x1b[1;1HB')

/** The per-frame content-change mode. `sparse` toggles one cell (typical), `full`
 *  rewrites every cell (worst case — forces a full GPU re-encode each frame, which
 *  is what the CPU path does unconditionally). */
export type AtermBenchMutation = 'sparse' | 'full'

type EngineLike = {
  process: (b: Uint8Array) => void
  render: () => void
  free: () => void
}

/** Time `frames` draws of a built engine. `present` runs the per-frame present
 *  (CPU: render+blit; GPU: render+gl.finish). Mutates content each frame per
 *  `mutation`. Returns steady-state ms/frame + the first-frame (warm-up) cost. */
function timeFrames(
  engine: EngineLike,
  frames: number,
  cols: number,
  rows: number,
  mutation: AtermBenchMutation,
  present: () => void
): { totalMs: number; firstFrameMs: number } {
  const fullA = fillBytes(cols, rows)
  // Slightly shift the full-refill content so each frame's grid actually differs.
  const fullB = fillBytes(cols, rows)
  const last = fullB.length - 1
  fullB[last] = fullB.at(-1) === 65 ? 66 : 65
  const mutate = (i: number): void => {
    if (mutation === 'full') {
      engine.process(i % 2 === 0 ? fullA : fullB)
    } else {
      engine.process(i % 2 === 0 ? MUT_A : MUT_B)
    }
  }

  // First frame separately: warms font shaping / first GPU atlas+submit.
  const f0 = performance.now()
  present()
  const firstFrameMs = performance.now() - f0

  const start = performance.now()
  for (let i = 0; i < frames; i++) {
    mutate(i)
    present()
  }
  return { totalMs: performance.now() - start, firstFrameMs }
}

async function benchCpu(opts: {
  cols: number
  rows: number
  fontPx: number
  themeColors: AtermThemeColors
  frames: number
  mutation: AtermBenchMutation
}): Promise<AtermPathBenchResult> {
  const { cols, rows, fontPx, themeColors, frames, mutation } = opts
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
  // A real 2d canvas so the per-frame blit cost (putImageData of width*height*4
  // bytes) is included — the honest CPU per-frame cost, not just the wasm raster.
  const canvas = document.createElement('canvas')
  const ctx = canvas.getContext('2d')
  if (!ctx) {
    term.free()
    throw new Error('cpu bench: no 2d context')
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
    ctx.putImageData(new ImageData(new Uint8ClampedArray(term.rgba()), w, h), 0, 0)
  }

  const { totalMs, firstFrameMs } = timeFrames(term, frames, cols, rows, mutation, present)
  const width = term.width
  const height = term.height
  term.free()
  const msPerFrame = totalMs / frames
  return {
    path: 'cpu',
    mutation,
    cols,
    rows,
    frames,
    width,
    height,
    totalMs,
    msPerFrame,
    fps: msPerFrame > 0 ? 1000 / msPerFrame : 0,
    firstFrameMs,
    initMs: 0,
    submitMsPerFrame: 0
  }
}

async function benchGpu(opts: {
  cols: number
  rows: number
  fontPx: number
  themeColors: AtermThemeColors
  frames: number
  mutation: AtermBenchMutation
}): Promise<{ result: AtermPathBenchResult; adapterInfo: string | null }> {
  const { cols, rows, fontPx, themeColors, frames, mutation } = opts
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
  const initStart = performance.now()
  await gpu.init(canvas) // acquire GPU + configure the WebGL2 swapchain (one-time)
  const initMs = performance.now() - initStart

  // wgpu's present_input only submits + presents — it does NOT device.poll(Wait),
  // so on WebGL2/ANGLE the GL commands are merely QUEUED and render() returns
  // before the GPU finishes. Timing render() alone measures submission throughput
  // (absurd fps), not completed frames. gl.finish() blocks until the GPU has
  // executed every queued command, making each timed frame an HONEST end-to-end
  // present — the closest WebGL2 equivalent of the native path's poll(Wait).
  const gl = canvas.getContext('webgl2')
  if (!gl) {
    gpu.free()
    throw new Error('gpu bench: no webgl2 context to sync')
  }

  gpu.process(fillBytes(cols, rows))
  // First: time render() WITHOUT gl.finish() — the command-submission cost alone.
  // (No content sync here; this isolates submit overhead.)
  const submit = timeFrames(gpu, frames, cols, rows, mutation, () => gpu.render())
  gl.finish() // drain anything queued before the synced pass

  // Then: the HONEST per-frame present — render() + gl.finish() forces GPU
  // completion so each timed frame is a real, finished swapchain present (the
  // closest WebGL2 equivalent of the native path's device.poll(Wait)).
  const present = (): void => {
    gpu.render()
    gl.finish()
  }
  const { totalMs, firstFrameMs } = timeFrames(gpu, frames, cols, rows, mutation, present)
  const width = canvas.width
  const height = canvas.height
  const adapterInfo = gpu.adapter_info || null
  gpu.free()
  const msPerFrame = totalMs / frames
  return {
    result: {
      path: 'gpu',
      mutation,
      cols,
      rows,
      frames,
      width,
      height,
      totalMs,
      msPerFrame,
      fps: msPerFrame > 0 ? 1000 / msPerFrame : 0,
      firstFrameMs,
      initMs,
      submitMsPerFrame: submit.totalMs / frames
    },
    adapterInfo
  }
}

function probeGl(): { renderer: string | null; vendor: string | null } {
  try {
    const c = document.createElement('canvas')
    const gl = c.getContext('webgl2')
    if (!gl) {
      return { renderer: null, vendor: null }
    }
    const dbg = gl.getExtension('WEBGL_debug_renderer_info')
    const renderer = dbg ? String(gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL) ?? '') || null : null
    const vendor = dbg ? String(gl.getParameter(dbg.UNMASKED_VENDOR_WEBGL) ?? '') || null : null
    gl.getExtension('WEBGL_lose_context')?.loseContext()
    return { renderer, vendor }
  } catch {
    return { renderer: null, vendor: null }
  }
}

export async function benchAtermGpuVsCpu(opts: {
  sizes: [number, number][]
  frames: number
  fontPx: number
  themeColors: AtermThemeColors
}): Promise<AtermGpuCpuBenchResult> {
  const { sizes, frames, fontPx, themeColors } = opts
  const gl = probeGl()

  // One-time wasm module load+instantiate cost for each engine (loaders memoize,
  // so this only captures real work on the first call — done before any timing).
  const cpuLoadStart = performance.now()
  await loadAterm()
  const cpuWasmLoadMs = performance.now() - cpuLoadStart
  const gpuLoadStart = performance.now()
  await loadAtermGpu()
  const gpuWasmLoadMs = performance.now() - gpuLoadStart

  let adapterInfo: string | null = null
  let firstReason: string | undefined
  let anyGpu = false

  // One grid + one mutation mode: CPU pair, then the GPU pair (null on failure).
  const measureMode = async (
    cols: number,
    gridRows: number,
    mutation: AtermBenchMutation
  ): Promise<AtermBenchModeRow> => {
    const cpu = await benchCpu({ cols, rows: gridRows, fontPx, themeColors, frames, mutation })
    let gpu: AtermPathBenchResult | null = null
    try {
      const g = await benchGpu({ cols, rows: gridRows, fontPx, themeColors, frames, mutation })
      gpu = g.result
      adapterInfo ??= g.adapterInfo
      anyGpu = true
    } catch (err) {
      firstReason ??= `gpu bench failed at ${cols}x${gridRows} (${mutation}): ${String(err)}`
    }
    return { cols, rows: gridRows, mutation, cpu, gpu }
  }

  const rows: AtermBenchRow[] = []
  for (const [cols, gridRows] of sizes) {
    const sparse = await measureMode(cols, gridRows, 'sparse')
    const full = await measureMode(cols, gridRows, 'full')
    rows.push({ cols, rows: gridRows, sparse, full })
  }

  return {
    available: anyGpu,
    reason: anyGpu ? undefined : firstReason,
    rows,
    adapterInfo,
    glRenderer: gl.renderer,
    glVendor: gl.vendor,
    cpuWasmLoadMs,
    gpuWasmLoadMs
  }
}
