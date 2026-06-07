// TS dispatch for the feature-education-telemetry parity module: maps the
// shared vector function names to the real
// `src/shared/feature-education-telemetry.ts` exports so the harness compares
// the live TS reference against the Rust port.

import {
  normalizeFeatureEducationSource,
  normalizeSetupGuideSource
} from '../../../src/shared/feature-education-telemetry'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'normalizeFeatureEducationSource':
      return normalizeFeatureEducationSource(input as string | null | undefined)
    case 'normalizeSetupGuideSource':
      return normalizeSetupGuideSource(input as string | null | undefined)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
