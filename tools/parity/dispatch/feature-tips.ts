// TS dispatch for the feature-tips parity module: maps the shared vector
// function names to the real `src/shared/feature-tips.ts` exports so the
// harness compares the live TS reference against the Rust port.

import {
  getCompletedFeatureTipIds,
  getOrderedUnseenFeatureTips,
  isFeatureTipId,
  normalizeFeatureTipIds,
  type CompletedFeatureTipState,
  type FeatureTipId
} from '../../../src/shared/feature-tips'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'isFeatureTipId':
      return isFeatureTipId(input)
    case 'normalizeFeatureTipIds':
      return normalizeFeatureTipIds(input)
    case 'getCompletedFeatureTipIds':
      // Spread the Set to an array so it survives JSON.stringify.
      return [...getCompletedFeatureTipIds(input as CompletedFeatureTipState)]
    case 'getOrderedUnseenFeatureTips': {
      const { seenTipIds, completedTipIds } = input as {
        seenTipIds: FeatureTipId[]
        completedTipIds?: FeatureTipId[]
      }
      return getOrderedUnseenFeatureTips({
        seenTipIds: new Set(seenTipIds),
        completedTipIds: completedTipIds ? new Set(completedTipIds) : undefined
      })
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
