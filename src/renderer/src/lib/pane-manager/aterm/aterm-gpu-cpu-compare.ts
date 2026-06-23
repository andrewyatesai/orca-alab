import { loadAterm } from './load-aterm'
import { loadAtermGpu } from './load-aterm-gpu'
import type { AtermThemeColors } from './aterm-theme-colors'

/** e2e-only GPU-vs-CPU parity probe. Builds a FRESH CPU engine (`aterm-wasm`) and
 *  a fresh GPU engine (`aterm-gpu-web`) at the SAME grid + theme + font px, feeds
 *  both the SAME bytes, and compares pixels: the GPU presents to its WebGL2 canvas
 *  and we `gl.readPixels` the swapchain; the CPU rasterizes to its RGBA buffer.
 *  Proves the WebGL backend draws the grid to the same pixels as the gating CPU
 *  path. Uses readPixels (NOT render_offscreen) because WebGL2 can't block-poll a
 *  buffer map — the readback path that works on native deadlocks in the browser. */
export type AtermGpuCpuCompareResult = {
  available: boolean
  reason?: string
  width: number
  height: number
  maxChannelDiff: number
  /** Fraction of pixels whose max per-channel GPU↔CPU diff is within ±6 (1.0 =
   *  every pixel matches). The robust parity metric: two rasterizers agree on the
   *  vast majority of pixels even if a few glyph-edge AA pixels round differently. */
  withinToleranceFraction: number
  sampledPixels: number
  /** Non-background pixel counts, to prove neither frame was blank. */
  gpuNonBg: number
  cpuNonBg: number
}

const EMPTY = {
  width: 0,
  height: 0,
  maxChannelDiff: 0,
  withinToleranceFraction: 0,
  sampledPixels: 0,
  gpuNonBg: 0,
  cpuNonBg: 0
}

export async function compareAtermGpuVsCpu(opts: {
  rows: number
  cols: number
  fontPx: number
  themeColors: AtermThemeColors
  bytes: Uint8Array
  canvas: HTMLCanvasElement
}): Promise<AtermGpuCpuCompareResult> {
  const { rows, cols, fontPx, themeColors, bytes, canvas } = opts

  // Both loaders return the ctor + the SAME injected font bytes, so each engine
  // rasterizes from the identical font — a fair pixel comparison.
  const gpuModule = await loadAtermGpu()
  const cpuModule = await loadAterm()

  const gpu = new gpuModule.AtermGpuTerminal(
    rows,
    cols,
    gpuModule.fontBytes,
    fontPx,
    themeColors.fg,
    themeColors.bg,
    themeColors.cursor,
    themeColors.selection
  )
  try {
    await gpu.init(canvas)
  } catch (err) {
    gpu.free()
    return { available: false, reason: `gpu init failed: ${String(err)}`, ...EMPTY }
  }
  gpu.process(bytes)
  gpu.render() // present the grid into the WebGL2 swapchain

  // Read the swapchain back via the canvas's webgl2 context (the one wgpu owns;
  // getContext returns the existing context). wgpu's WebGL present already lands
  // the frame top-to-bottom in the same orientation as the CPU's RGBA buffer
  // (verified by the row-alignment probe: flip=false,shift=0 is the exact match),
  // so NO vertical flip is applied here.
  const gl = canvas.getContext('webgl2')
  if (!gl || !canvas.width || !canvas.height) {
    gpu.free()
    return { available: false, reason: 'no webgl2 context to read back', ...EMPTY }
  }
  const width = canvas.width
  const height = canvas.height
  const gpuPixels = new Uint8Array(width * height * 4)
  gl.readPixels(0, 0, width, height, gl.RGBA, gl.UNSIGNED_BYTE, gpuPixels)

  const cpu = new cpuModule.AtermTerminal(
    rows,
    cols,
    cpuModule.fontBytes,
    fontPx,
    themeColors.fg,
    themeColors.bg,
    themeColors.cursor,
    themeColors.selection
  )
  cpu.process(bytes)
  cpu.render()
  const cpuRgba = cpu.rgba()

  if (width === 0 || height === 0 || cpu.width !== width || cpu.height !== height) {
    const reason = `frame size mismatch gpu=${width}x${height} cpu=${cpu.width}x${cpu.height}`
    gpu.free()
    cpu.free()
    return { available: false, reason, ...EMPTY }
  }

  // The seeded default bg (themeColors.bg, 0x00RRGGBB) — the canonical empty-cell
  // color. Using a fixed reference (not the top-left pixel, which may carry a
  // glyph/cursor) keeps the non-bg counts meaningful.
  const bgR = (themeColors.bg >> 16) & 0xff
  const bgG = (themeColors.bg >> 8) & 0xff
  const bgB = themeColors.bg & 0xff
  let maxChannelDiff = 0
  let gpuNonBg = 0
  let cpuNonBg = 0
  // Per-pixel max-channel diff distribution: the overwhelming majority of pixels
  // must match within ±6 between the two rasterizers; a handful of glyph-edge
  // antialiasing pixels can differ more (two backends round sub-pixel coverage
  // independently), so we report the within-tolerance FRACTION too.
  let withinTolerance = 0
  const TOLERANCE = 6
  for (let i = 0; i < width * height * 4; i += 4) {
    let pixMax = 0
    for (let ch = 0; ch < 3; ch++) {
      const d = Math.abs(gpuPixels[i + ch] - cpuRgba[i + ch])
      if (d > pixMax) {
        pixMax = d
      }
    }
    if (pixMax > maxChannelDiff) {
      maxChannelDiff = pixMax
    }
    if (pixMax <= TOLERANCE) {
      withinTolerance++
    }
    if (gpuPixels[i] !== bgR || gpuPixels[i + 1] !== bgG || gpuPixels[i + 2] !== bgB) {
      gpuNonBg++
    }
    if (cpuRgba[i] !== bgR || cpuRgba[i + 1] !== bgG || cpuRgba[i + 2] !== bgB) {
      cpuNonBg++
    }
  }
  const total = width * height
  const withinToleranceFraction = total > 0 ? withinTolerance / total : 0

  gpu.free()
  cpu.free()
  return {
    available: true,
    width,
    height,
    maxChannelDiff,
    withinToleranceFraction,
    sampledPixels: total,
    gpuNonBg,
    cpuNonBg
  }
}
