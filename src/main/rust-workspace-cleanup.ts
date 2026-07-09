// Main-process workspace-cleanup classifiers, driven by the Rust
// workspace-cleanup core via napi (the shared TS impl was gutted to types +
// data). One source of truth with the parity-proven Rust port; main's candidate
// builders consume the policy/fingerprint/inactivity helpers, so those are the
// exports here.
import { requireRustGitBinding } from './daemon/rust-git-addon'
import type {
  WorkspaceCleanupCandidate,
  WorkspaceCleanupInactivityInput,
  WorkspaceCleanupReason
} from '../shared/workspace-cleanup'

function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(
    requireRustGitBinding().orcaDispatch('workspace-cleanup', fn, JSON.stringify(input ?? null))
  )
}

export function applyWorkspaceCleanupPolicy(
  candidate: WorkspaceCleanupCandidate
): WorkspaceCleanupCandidate {
  return dispatch('applyWorkspaceCleanupPolicy', candidate) as WorkspaceCleanupCandidate
}

export function createWorkspaceCleanupFingerprint(args: {
  branch: string
  head: string
  gitClean: boolean | null
  lastActivityAt: number
  classifierVersion?: number
}): string {
  return dispatch('createWorkspaceCleanupFingerprint', args) as string
}

export function getWorkspaceCleanupInactivityReasons(
  workspace: WorkspaceCleanupInactivityInput,
  scannedAt: number
): WorkspaceCleanupReason[] {
  return dispatch('getWorkspaceCleanupInactivityReasons', {
    workspace,
    scannedAt
  }) as WorkspaceCleanupReason[]
}

export function isWorkspaceOldForCleanup(
  workspace: WorkspaceCleanupInactivityInput,
  scannedAt: number
): boolean {
  return dispatch('isWorkspaceOldForCleanup', { workspace, scannedAt }) as boolean
}
