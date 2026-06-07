// TS dispatch for the tab-title-resolution parity module: maps the shared
// vector function names to the real `src/shared/tab-title-resolution.ts`
// exports so the harness compares the live TS reference against the Rust port.

import {
  resolveTerminalTabTitle,
  resolveUnifiedTabLabel
} from '../../../src/shared/tab-title-resolution'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'resolveTerminalTabTitle': {
      const { tab, generatedTitlesEnabled, fallback } = input as {
        tab: Parameters<typeof resolveTerminalTabTitle>[0]
        generatedTitlesEnabled: boolean
        fallback?: string
      }
      return resolveTerminalTabTitle(tab, generatedTitlesEnabled, fallback)
    }
    case 'resolveUnifiedTabLabel': {
      const { tab, generatedTitlesEnabled, fallback } = input as {
        tab: Parameters<typeof resolveUnifiedTabLabel>[0]
        generatedTitlesEnabled: boolean
        fallback?: string
      }
      return resolveUnifiedTabLabel(tab, generatedTitlesEnabled, fallback)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
