// TS dispatch for the workspace-cleanup parity module: maps the shared vector
// function names to the real `src/shared/workspace-cleanup.ts` exports so the
// harness compares the live TS reference against the Rust port.

import {
  applyWorkspaceCleanupPolicy,
  canQueueWorkspaceCleanupCandidate,
  canSelectWorkspaceCleanupCandidate,
  createWorkspaceCleanupFingerprint,
  getWorkspaceCleanupInactivityReasons,
  isWorkspaceCleanupHardBlocker,
  shouldForceWorkspaceCleanupRemoval,
  shouldHideWorkspaceCleanupCandidate,
  type WorkspaceCleanupBlocker,
  type WorkspaceCleanupCandidate,
  type WorkspaceCleanupDismissal,
  type WorkspaceCleanupInactivityInput
} from '../../../src/shared/workspace-cleanup'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'isWorkspaceCleanupHardBlocker':
      return isWorkspaceCleanupHardBlocker(input as WorkspaceCleanupBlocker)
    case 'canQueueWorkspaceCleanupCandidate':
      return canQueueWorkspaceCleanupCandidate(input as WorkspaceCleanupCandidate)
    case 'shouldForceWorkspaceCleanupRemoval':
      return shouldForceWorkspaceCleanupRemoval(input as WorkspaceCleanupCandidate)
    case 'canSelectWorkspaceCleanupCandidate':
      return canSelectWorkspaceCleanupCandidate(input as WorkspaceCleanupCandidate)
    case 'applyWorkspaceCleanupPolicy':
      return applyWorkspaceCleanupPolicy(input as WorkspaceCleanupCandidate)
    case 'createWorkspaceCleanupFingerprint':
      return createWorkspaceCleanupFingerprint(
        input as Parameters<typeof createWorkspaceCleanupFingerprint>[0]
      )
    case 'getWorkspaceCleanupInactivityReasons': {
      const { workspace, scannedAt } = input as {
        workspace: WorkspaceCleanupInactivityInput
        scannedAt: number
      }
      return getWorkspaceCleanupInactivityReasons(workspace, scannedAt)
    }
    case 'shouldHideWorkspaceCleanupCandidate': {
      const { candidate, dismissal } = input as {
        candidate: WorkspaceCleanupCandidate
        dismissal: WorkspaceCleanupDismissal | null | undefined
      }
      return shouldHideWorkspaceCleanupCandidate(candidate, dismissal ?? undefined)
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
