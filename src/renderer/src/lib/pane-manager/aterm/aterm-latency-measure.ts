import type { LatencyStats } from './aterm-latency-bench-types'

// Shared measurement primitives for the keystroke-latency benchmark: the
// single-cell update payloads, the dense fill, the stats reducer, the GL-string
// probe, and the per-frame timing loop. Kept renderer-agnostic so the aterm and
// xterm benches share exactly the same sampling.

// Single-cell mutation at the home position, alternating so the grid changes every
// iteration (no engine can short-circuit an unchanged frame). This is the canonical
// "one keystroke echoed" update: one printable glyph lands at the cursor.
export const CELL_A = new TextEncoder().encode('\x1b[1;1HA')
export const CELL_B = new TextEncoder().encode('\x1b[1;1HB')

export function summarize(samplesMs: number[]): LatencyStats {
  const sorted = [...samplesMs].sort((a, b) => a - b)
  const n = sorted.length
  const pick = (q: number): number => sorted[Math.min(n - 1, Math.floor(q * n))] ?? 0
  const sum = sorted.reduce((acc, v) => acc + v, 0)
  return {
    samples: n,
    medianMs: pick(0.5),
    p95Ms: pick(0.95),
    minMs: sorted[0] ?? 0,
    maxMs: sorted[n - 1] ?? 0,
    meanMs: n > 0 ? sum / n : 0
  }
}

/** Dense per-cell SGR fill so the rasterizer does representative work (matches the
 *  existing GPU/CPU frame bench fill). */
export function fillBytes(cols: number, rows: number): Uint8Array {
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

export function probeGl(): { renderer: string | null; vendor: string | null } {
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

export type EngineFrame = {
  process: (b: Uint8Array) => void
  render: () => void
  free: () => void
}

/** Time `frames` single-cell updates of a built engine; `present` runs the
 *  per-frame present (CPU render+blit, or GPU render+finish). Returns ms/frame. */
export function timeEngineFrames(
  engine: EngineFrame,
  frames: number,
  present: () => void
): number {
  // Warm the first frame (font shaping / GPU atlas) outside the timed loop.
  present()
  const start = performance.now()
  for (let i = 0; i < frames; i++) {
    engine.process(i % 2 === 0 ? CELL_A : CELL_B)
    present()
  }
  return (performance.now() - start) / frames
}
