// TS dispatch for the browser-viewport-presets parity module: maps the shared
// vector function names to the real `src/shared/browser-viewport-presets.ts`
// exports so the harness compares the live TS reference against the Rust port.

import {
  browserViewportPresetToOverride,
  getBrowserViewportPreset,
  type BrowserViewportPreset
} from '../../../src/shared/browser-viewport-presets'
import type { BrowserViewportPresetId } from '../../../src/shared/types'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'getBrowserViewportPreset':
      return getBrowserViewportPreset(input as BrowserViewportPresetId | null)
    case 'browserViewportPresetToOverride':
      return browserViewportPresetToOverride(input as BrowserViewportPreset)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
