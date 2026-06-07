// TS dispatch for the hosted-review-refs parity module: maps the shared vector
// function names to the real `src/shared/hosted-review-refs.ts` exports so the
// harness compares the live TS reference against the Rust port.

import {
  normalizeHostedReviewBaseRef,
  normalizeHostedReviewHeadRef
} from '../../../src/shared/hosted-review-refs'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'normalizeHostedReviewHeadRef':
      return normalizeHostedReviewHeadRef(input as string)
    case 'normalizeHostedReviewBaseRef':
      return normalizeHostedReviewBaseRef(input as string)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
