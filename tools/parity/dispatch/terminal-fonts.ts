// TS dispatch for the terminal-fonts parity module: maps the shared vector
// function names to the real `src/shared/terminal-fonts.ts` exports so the
// harness compares the live TS reference against the Rust port.

import {
  normalizeTerminalFontWeight,
  resolveTerminalFontWeights
} from '../../../src/shared/terminal-fonts'

export function dispatch(fn: string, input: unknown): unknown {
  // Both functions take a single numeric arg; JSON null carries TS null/undefined.
  const fontWeight = input as number | null | undefined
  switch (fn) {
    case 'normalizeTerminalFontWeight':
      return normalizeTerminalFontWeight(fontWeight)
    case 'resolveTerminalFontWeights':
      return resolveTerminalFontWeights(fontWeight)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
