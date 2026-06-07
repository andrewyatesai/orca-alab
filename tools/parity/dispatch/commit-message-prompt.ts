// TS dispatch for the commit-message-prompt parity module: maps the shared
// vector function names to the real `src/shared/commit-message-prompt.ts`
// exports so the harness compares the live TS reference against the Rust port.
// Only the pure transforms are covered here.

import {
  buildCommitPrompt,
  cleanGeneratedCommitMessage
} from '../../../src/shared/commit-message-prompt'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'buildCommitPrompt': {
      const { diff, suffix } = input as { diff: string; suffix: string }
      return buildCommitPrompt(diff, suffix)
    }
    case 'cleanGeneratedCommitMessage':
      return cleanGeneratedCommitMessage(input as string)
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
