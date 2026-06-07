// TS dispatch for the hosted-review-queue parity module: maps the shared vector
// function names to the real `src/shared/hosted-review-queue.ts` exports so the
// harness compares the live TS reference against the Rust port.

import {
  hostedReviewIdentityKey,
  reviewNeedsResponse,
  reviewReadyToMerge
} from '../../../src/shared/hosted-review-queue'
import type {
  HostedReviewIdentity,
  HostedReviewQueueSummary
} from '../../../src/shared/hosted-review'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'hostedReviewIdentityKey':
      return hostedReviewIdentityKey(input as HostedReviewIdentity)
    case 'reviewNeedsResponse':
      // viewer is unused by the implementation, so the vector carries only the summary.
      return reviewNeedsResponse(input as HostedReviewQueueSummary)
    case 'reviewReadyToMerge':
      return reviewReadyToMerge(input as HostedReviewQueueSummary)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
