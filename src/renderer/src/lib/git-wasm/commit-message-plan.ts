// Renderer commit-message planner, driven by the Rust orca-agents core in the
// orca-git wasm module (the shared TS bodies were deleted). The renderer's only
// use is a dry-run planner PREVIEW dialog, so pre-ready the plan is null and the
// dialog's Run stays disabled until the wasm initialises — the authoritative
// planner runs in the main process (napi).
import {
  planAgentBinary as wasmPlanAgentBinary,
  planCommitMessageGeneration as wasmPlanCommitMessageGeneration
} from './orca_git_wasm.js'
import { isGitWasmReady } from './git-line-stats'
import type {
  CommitMessagePlanInput,
  CommitMessagePlanResult,
  PlanAgentBinaryResult
} from '../../../../shared/commit-message-plan'

export function planCommitMessageGeneration(
  input: CommitMessagePlanInput,
  prompt: string
): CommitMessagePlanResult | null {
  if (!isGitWasmReady()) {
    return null
  }
  return JSON.parse(
    wasmPlanCommitMessageGeneration(JSON.stringify(input), prompt)
  ) as CommitMessagePlanResult
}

export function planAgentBinary(
  defaultBinary: string,
  commandOverride: string | undefined
): PlanAgentBinaryResult | null {
  if (!isGitWasmReady()) {
    return null
  }
  return JSON.parse(
    wasmPlanAgentBinary(defaultBinary, commandOverride)
  ) as PlanAgentBinaryResult
}
