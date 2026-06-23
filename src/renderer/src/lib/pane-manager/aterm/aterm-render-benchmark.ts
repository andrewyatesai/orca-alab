import type { AtermTerminal } from './aterm_wasm.js'

export type AtermRenderBenchmarkResult = {
  cols: number
  rows: number
  frames: number
  totalMs: number
  msPerFrame: number
  fps: number
}

/** Time the real in-wasm rasterizer at a given grid. Fills every cell with mixed
 *  SGR-colored content (representative per-cell work, not a blank frame), warms
 *  up once, then times `frames` pure render() calls — toggling a cell each
 *  iteration so the engine can't no-op an unchanged grid. Restores the live grid
 *  afterward so the visible pane is unperturbed. e2e/perf-only. */
export function benchmarkAtermRender(
  term: AtermTerminal,
  live: { cols: number; rows: number },
  benchCols: number,
  benchRows: number,
  frames: number,
  scheduleDraw: () => void
): AtermRenderBenchmarkResult {
  const encoder = new TextEncoder()
  const line = (row: number): string => {
    let s = `\x1b[${(row % 7) + 31}m`
    for (let c = 0; c < benchCols; c++) {
      s += String.fromCharCode(33 + ((row * 7 + c) % 94))
    }
    return `${s}\x1b[0m`
  }
  try {
    term.resize(benchRows, benchCols)
    const fill = `\x1b[H${Array.from({ length: benchRows }, (_, r) => line(r)).join('\r\n')}`
    term.process(encoder.encode(fill))
    term.render() // warm up (font shaping / first-frame allocations)
    const start = performance.now()
    for (let i = 0; i < frames; i++) {
      term.process(encoder.encode(`\x1b[1;1H${i % 2 === 0 ? 'A' : 'B'}`))
      term.render()
    }
    const totalMs = performance.now() - start
    const msPerFrame = totalMs / frames
    return {
      cols: benchCols,
      rows: benchRows,
      frames,
      totalMs,
      msPerFrame,
      fps: msPerFrame > 0 ? 1000 / msPerFrame : 0
    }
  } finally {
    term.resize(live.rows, live.cols)
    scheduleDraw()
  }
}
