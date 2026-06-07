// TS dispatch for the commit-message-plan parity module: maps the shared vector
// function names to the real `src/shared/commit-message-plan.ts` exports so the
// harness compares the live TS reference against the Rust port.

import {
  planCommitMessageGeneration,
  type CommitMessagePlanInput
} from '../../../src/shared/commit-message-plan'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'planCommitMessageGeneration': {
      const { planInput, prompt } = input as { planInput: CommitMessagePlanInput; prompt: string }
      return planCommitMessageGeneration(planInput, prompt)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
