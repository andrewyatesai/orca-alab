// Types for the commit-message generator. The prompt-build + generated-message
// split bodies were DELETED — the Rust `orca-agents::commit_message_generation`
// core is the sole impl (napi in main via ./rust-commit-message-generation, wasm
// in the renderer's dialog preview). See that crate + the parity vectors.
import type { TuiAgent } from './types'

export type CommitMessageDraftAgent = TuiAgent | 'custom'

export type CommitMessageDraftContext = {
  branch: string | null
  stagedSummary: string
  stagedPatch: string
}

export type CommitMessageDraftOptions = {
  agentId: CommitMessageDraftAgent
  model: string
  thinkingLevel?: string
  customPrompt?: string
  customAgentCommand?: string
}

export type GeneratedCommitMessage = {
  subject: string
  body: string
  message: string
}
