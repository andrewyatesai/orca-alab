// TS dispatch for the pull-request-generation parity module: maps the shared
// vector function names to the real `src/shared/pull-request-generation.ts`
// exports so the harness compares the live TS reference against the Rust port.

import {
  buildPullRequestFieldsPrompt,
  type PullRequestDraftContext
} from '../../../src/shared/pull-request-generation'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'buildPullRequestFieldsPrompt': {
      const { context, customPrompt } = input as {
        context: PullRequestDraftContext
        customPrompt: string
      }
      return buildPullRequestFieldsPrompt(context, customPrompt)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
