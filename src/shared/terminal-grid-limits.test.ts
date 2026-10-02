import { describe, expect, it } from 'vitest'
import { capTerminalGridDim, TERMINAL_GRID_MAX_DIM } from './terminal-grid-limits'

describe('capTerminalGridDim', () => {
  it('matches the Rust daemon cap (orca-daemon rpc.rs MAX_GRID_DIM)', () => {
    expect(TERMINAL_GRID_MAX_DIM).toBe(4096)
  })

  it('caps above 4096 and passes everything else through unchanged', () => {
    expect(capTerminalGridDim(4097)).toBe(4096)
    expect(capTerminalGridDim(65_535)).toBe(4096)
    expect(capTerminalGridDim(4096)).toBe(4096)
    expect(capTerminalGridDim(80)).toBe(80)
    expect(capTerminalGridDim(1)).toBe(1)
    expect(capTerminalGridDim(0)).toBe(0)
    expect(capTerminalGridDim(-1)).toBe(-1)
    expect(capTerminalGridDim(Number.NaN)).toBeNaN()
    // Non-finite stays non-finite so normalizePtySize still maps it to 80x24.
    expect(capTerminalGridDim(Number.POSITIVE_INFINITY)).toBe(Number.POSITIVE_INFINITY)
  })
})
