// TS dispatch for the commit-message-generation parity module: maps the shared
// vector function names to the real `src/shared/commit-message-generation.ts`
// exports so the harness compares the live TS reference against the Rust port.

import {
  buildCommitMessagePrompt,
  splitGeneratedCommitMessage,
  type CommitMessageDraftContext
} from '../../../src/shared/commit-message-generation'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'buildCommitMessagePrompt': {
      const { context, customPrompt } = input as {
        context: CommitMessageDraftContext
        customPrompt: string
      }
      return buildCommitMessagePrompt(context, customPrompt)
    }
    case 'splitGeneratedCommitMessage':
      return splitGeneratedCommitMessage(input as string)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
