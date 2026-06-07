// TS dispatch for the feature-interactions parity module: maps the shared
// vector function names to the real `src/shared/feature-interactions.ts`
// exports so the harness compares the live TS reference against the Rust port.

import {
  hasFeatureInteraction,
  isFeatureInteractionId,
  normalizeFeatureInteractions,
  type FeatureInteractionId
} from '../../../src/shared/feature-interactions'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'normalizeFeatureInteractions':
      return normalizeFeatureInteractions(input)
    case 'hasFeatureInteraction': {
      const { state, id } = input as { state?: unknown; id: FeatureInteractionId }
      return hasFeatureInteraction(state as never, id)
    }
    case 'isFeatureInteractionId':
      return isFeatureInteractionId(input)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
