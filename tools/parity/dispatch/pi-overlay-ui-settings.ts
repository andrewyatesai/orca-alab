// TS dispatch for the pi-overlay-ui-settings parity module: maps the shared
// vector function names to the real `src/shared/pi-overlay-ui-settings.ts`
// exports so the harness compares the live TS reference against the Rust port.

import { mergePiOverlayUiSettings } from '../../../src/shared/pi-overlay-ui-settings'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'mergePiOverlayUiSettings':
      return mergePiOverlayUiSettings(input)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
