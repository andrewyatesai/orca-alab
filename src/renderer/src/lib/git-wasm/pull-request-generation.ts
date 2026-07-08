// Renderer pull-request prompt builder, driven by the Rust orca-agents core in
// the orca-git wasm module (the shared TS bodies were deleted). The renderer's
// only use is the dry-run generation-dialog PREVIEW, so pre-ready the prompt is
// null and the caller shows nothing until the wasm initialises — the
// authoritative generator runs in the main process (napi).
import { buildPullRequestFieldsPrompt as wasmBuildPullRequestFieldsPrompt } from './orca_git_wasm.js'
import { isGitWasmReady } from './git-line-stats'
import type { PullRequestDraftContext } from '../../../../shared/pull-request-generation'

export function buildPullRequestFieldsPrompt(
  context: PullRequestDraftContext,
  customPrompt: string
): string | null {
  if (!isGitWasmReady()) {
    return null
  }
  return wasmBuildPullRequestFieldsPrompt(JSON.stringify(context), customPrompt)
}
