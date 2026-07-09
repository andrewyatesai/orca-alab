// Renderer commit-message prompt builder, driven by the Rust orca-agents core in
// the orca-git wasm module (the shared TS body was deleted). The renderer's only
// use is the generation-dialog PREVIEW, so pre-ready the prompt is null and the
// caller shows nothing until the wasm initialises — the authoritative generator
// runs in the main process (napi).
import { isGitWasmReady } from './git-line-stats'
import { orcaDispatch } from './orca_git_wasm.js'
import type { CommitMessageDraftContext } from '../../../../shared/commit-message-generation'

function op(fn: string, input: unknown): unknown {
  if (!isGitWasmReady()) {
    return null
  }
  return JSON.parse(orcaDispatch('commit-message-generation', fn, JSON.stringify(input ?? null)))
}

export function buildCommitMessagePrompt(
  context: CommitMessageDraftContext,
  customPrompt: string
): string | null {
  return op('buildCommitMessagePrompt', { context, customPrompt }) as string | null
}
