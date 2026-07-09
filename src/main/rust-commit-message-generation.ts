// Main-process commit-message prompt builder + generated-message splitter, driven
// by the Rust orca-agents core via napi (the shared TS bodies were deleted). One
// source of truth with the parity-proven Rust port — the dispatch composes the
// diff truncation and output cleaning internally.
import { requireRustGitBinding } from './daemon/rust-git-addon'
import type {
  CommitMessageDraftContext,
  GeneratedCommitMessage
} from '../shared/commit-message-generation'

export function buildCommitMessagePrompt(
  context: CommitMessageDraftContext,
  customPrompt: string
): string {
  return JSON.parse(
    requireRustGitBinding().orcaDispatch(
      'commit-message-generation',
      'buildCommitMessagePrompt',
      JSON.stringify({ context, customPrompt })
    )
  ) as string
}

export function splitGeneratedCommitMessage(message: string): GeneratedCommitMessage {
  // Rust reads the raw message via `input.as_str()`, so send the bare string.
  return JSON.parse(
    requireRustGitBinding().orcaDispatch(
      'commit-message-generation',
      'splitGeneratedCommitMessage',
      JSON.stringify(message)
    )
  ) as GeneratedCommitMessage
}
