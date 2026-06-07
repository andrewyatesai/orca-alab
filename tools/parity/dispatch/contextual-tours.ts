// TS dispatch for the contextual-tours parity module: maps the shared vector
// function names to the real `src/shared/contextual-tours.ts` exports so the
// harness compares the live TS reference against the Rust port.

import {
  getContextualTour,
  isContextualTourId,
  normalizeContextualTourIds,
  type ContextualTourId
} from '../../../src/shared/contextual-tours'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'isContextualTourId':
      return isContextualTourId(input)
    case 'normalizeContextualTourIds':
      return normalizeContextualTourIds(input)
    case 'getContextualTour':
      return getContextualTour(input as ContextualTourId)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
