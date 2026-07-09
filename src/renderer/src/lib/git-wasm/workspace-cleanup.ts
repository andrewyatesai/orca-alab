// Renderer workspace-cleanup classifiers, driven by the Rust workspace-cleanup
// core in the orca-git wasm module (the shared TS impl was gutted to types +
// data). These run in sync cleanup reducers and the resource status-bar render,
// so every export returns a NON-NULL fallback when the wasm hasn't loaded yet.
// Booleans default to `false` ("no cleanup action") — the safe direction that
// never force-removes a workspace or hides a candidate before the core is ready.
import { isGitWasmReady } from './git-line-stats'
import { orcaDispatch } from './orca_git_wasm.js'
import type {
  WorkspaceCleanupCandidate,
  WorkspaceCleanupDismissal,
  WorkspaceCleanupInactivityInput
} from '../../../../shared/workspace-cleanup'

function op(fn: string, input: unknown): unknown | null {
  if (!isGitWasmReady()) return null
  return JSON.parse(orcaDispatch('workspace-cleanup', fn, JSON.stringify(input ?? null)))
}

export function canQueueWorkspaceCleanupCandidate(
  candidate: Pick<WorkspaceCleanupCandidate, 'blockers' | 'reasons'>
): boolean {
  return (op('canQueueWorkspaceCleanupCandidate', candidate) as boolean | null) ?? false
}

export function shouldForceWorkspaceCleanupRemoval(
  candidate: Pick<WorkspaceCleanupCandidate, 'blockers' | 'git'>
): boolean {
  return (op('shouldForceWorkspaceCleanupRemoval', candidate) as boolean | null) ?? false
}

export function canSelectWorkspaceCleanupCandidate(
  candidate: Pick<WorkspaceCleanupCandidate, 'blockers' | 'git' | 'reasons'>
): boolean {
  return (op('canSelectWorkspaceCleanupCandidate', candidate) as boolean | null) ?? false
}

export function applyWorkspaceCleanupPolicy(
  candidate: WorkspaceCleanupCandidate
): WorkspaceCleanupCandidate {
  // Pass the candidate through unchanged on wasm-load failure so the reducer
  // keeps its existing tier/selection instead of a recomputed-to-default one.
  const r = op('applyWorkspaceCleanupPolicy', candidate) as WorkspaceCleanupCandidate | null
  return r ?? candidate
}

export function isWorkspaceOldForCleanup(
  workspace: WorkspaceCleanupInactivityInput,
  scannedAt: number
): boolean {
  return (op('isWorkspaceOldForCleanup', { workspace, scannedAt }) as boolean | null) ?? false
}

export function shouldHideWorkspaceCleanupCandidate(
  candidate: Pick<WorkspaceCleanupCandidate, 'worktreeId' | 'fingerprint'>,
  dismissal: WorkspaceCleanupDismissal | undefined
): boolean {
  return (
    (op('shouldHideWorkspaceCleanupCandidate', { candidate, dismissal }) as boolean | null) ?? false
  )
}
