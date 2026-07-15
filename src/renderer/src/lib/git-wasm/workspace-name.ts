// Workspace name/seed derivation, driven by the Rust orca-text core in the
// orca-git wasm module (the shared TS implementation was deleted). These are
// RENDERER preview/seed helpers: the main process runs the authoritative
// worktree-name sanitizer at create time, and every consumer already falls
// back to a valid seed, so an empty/null during the ~tens-of-ms wasm boot
// window degrades to a less-descriptive (never broken) name.
import {
  getLinearIssueWorkspaceName as wasmLinearIssueName,
  getLinkedWorkItemSuggestedName as wasmSuggestedName,
  getLinkedWorkItemWorkspaceName as wasmLinkedWorkspaceName,
  getWorkspaceIntentName as wasmIntentName,
  slugifyForWorkspaceName as wasmSlugify
} from './orca_git_wasm.js'
import { isGitWasmReady } from './git-line-stats'
import type {
  WorkspaceIntentName,
  WorkspaceIntentWorkItem
} from '../../../../shared/workspace-name'
import {
  getWorkspaceSourceProvider,
  type WorkspaceSourceItemLike
} from '../../../../shared/new-workspace/workspace-source'

export function slugifyForWorkspaceName(input: string): string {
  if (!isGitWasmReady()) {
    return ''
  }
  return wasmSlugify(input)
}

export function getLinkedWorkItemSuggestedName(item: { title: string }): string {
  if (!isGitWasmReady()) {
    return ''
  }
  return wasmSuggestedName(item.title)
}

export function getLinearIssueWorkspaceName(issue: { identifier: string; title: string }): string {
  if (!isGitWasmReady()) {
    return ''
  }
  return wasmLinearIssueName(issue.identifier, issue.title)
}

export function getLinkedWorkItemWorkspaceName(
  item: WorkspaceIntentWorkItem
): WorkspaceIntentName | null {
  if (!isGitWasmReady()) {
    return null
  }
  const json = wasmLinkedWorkspaceName(JSON.stringify(item))
  return json === undefined ? null : (JSON.parse(json) as WorkspaceIntentName)
}

export function getWorkspaceIntentName(args: {
  sourceText?: string
  workItem?: WorkspaceIntentWorkItem | null
  fallbackName?: string
}): WorkspaceIntentName | null {
  if (!isGitWasmReady()) {
    return null
  }
  const json = wasmIntentName(JSON.stringify(args))
  return json === undefined ? null : (JSON.parse(json) as WorkspaceIntentName)
}

// Why here (renderer wasm) and not in the shared workspace-source module: the
// seed/display derivation runs through the Rust orca-text core via wasm, which is
// renderer-only. The shared module keeps the pure source policy; this preview
// helper composes the linked-item name from the wasm-backed derivations.
export function getWorkspaceSourceName(item: WorkspaceSourceItemLike): {
  seedName: string
  displayName: string
} {
  const normalized: WorkspaceIntentWorkItem = {
    ...item,
    provider: getWorkspaceSourceProvider(item)
  }
  const resolved = getLinkedWorkItemWorkspaceName(normalized)
  return {
    seedName: resolved?.seedName ?? getLinkedWorkItemSuggestedName(normalized),
    displayName: resolved?.displayName ?? item.title.trim()
  }
}
