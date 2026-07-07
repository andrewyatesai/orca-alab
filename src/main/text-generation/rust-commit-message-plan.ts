// Main-process commit-message planner, driven by the Rust orca-agents core via
// napi (the shared TS bodies were deleted). Same planner the renderer previews
// through wasm and the SSH provider ships to the relay to execute — one source
// of truth for the spawn binary + argv + stdin contract.
import { requireRustGitBinding } from '../daemon/rust-git-addon'
import type {
  CommitMessagePlanInput,
  CommitMessagePlanResult,
  PlanAgentBinaryResult
} from '../../shared/commit-message-plan'

export function planCommitMessageGeneration(
  input: CommitMessagePlanInput,
  prompt: string
): CommitMessagePlanResult {
  return JSON.parse(
    requireRustGitBinding().planCommitMessageGeneration(JSON.stringify(input), prompt)
  ) as CommitMessagePlanResult
}

export function planAgentBinary(
  defaultBinary: string,
  commandOverride: string | undefined
): PlanAgentBinaryResult {
  return JSON.parse(
    requireRustGitBinding().planAgentBinary(defaultBinary, commandOverride)
  ) as PlanAgentBinaryResult
}
