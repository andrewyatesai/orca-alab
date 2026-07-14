// Logic moved to the Rust workspace-cleanup core (orca-dispatch); this file retains types + data only.
export const WORKSPACE_CLEANUP_CLASSIFIER_VERSION = 2
export const WORKSPACE_CLEANUP_ARCHIVED_IDLE_MS = 7 * 24 * 60 * 60 * 1000
export const WORKSPACE_CLEANUP_IDLE_MS = 30 * 24 * 60 * 60 * 1000

export type WorkspaceCleanupTier = 'ready' | 'review' | 'protected'

export type WorkspaceCleanupReason = 'archived' | 'idle-clean'

export type WorkspaceCleanupInactivityInput = {
  isArchived: boolean
  lastActivityAt: number
}

export type WorkspaceCleanupBlocker =
  | 'main-worktree'
  | 'folder-repo'
  | 'pinned'
  | 'active-workspace'
  | 'running-terminal'
  | 'terminal-liveness-unknown'
  | 'dirty-editor-buffer'
  | 'volatile-local-context'
  | 'recent-visible-context'
  | 'live-agent'
  | 'ssh-disconnected'
  | 'git-status-error'
  | 'dirty-files'
  | 'unpushed-commits'
  | 'unknown-base'
  | 'dismissed'

export type WorkspaceCleanupDismissal = {
  worktreeId: string
  dismissedAt: number
  fingerprint: string
  classifierVersion: number
}

export type WorkspaceCleanupUIState = {
  dismissals: Record<string, WorkspaceCleanupDismissal>
}

export type WorkspaceCleanupCandidate = {
  worktreeId: string
  repoId: string
  repoName: string
  connectionId: string | null
  displayName: string
  branch: string
  path: string
  tier: WorkspaceCleanupTier
  selectedByDefault: boolean
  reasons: WorkspaceCleanupReason[]
  blockers: WorkspaceCleanupBlocker[]
  lastActivityAt: number
  createdAt?: number
  localContext: {
    terminalTabCount: number
    cleanEditorTabCount: number
    browserTabCount: number
    diffCommentCount: number
    newestDiffCommentAt: number | null
    retainedDoneAgentCount: number
  }
  git: {
    clean: boolean | null
    upstreamAhead: number | null
    upstreamBehind: number | null
    checkedAt: number | null
  }
  fingerprint: string
}

export type WorkspaceCleanupScanArgs = {
  worktreeId?: string
  skipGitWorktreeIds?: string[]
  scanId?: string
}

export type WorkspaceCleanupLocalProcessArgs = {
  worktreeId: string
  connectionId?: string | null
  worktreePath?: string
}

export type WorkspaceCleanupScanError = {
  repoId: string
  repoName: string
  message: string
}

export type WorkspaceCleanupScanResult = {
  scannedAt: number
  candidates: WorkspaceCleanupCandidate[]
  errors: WorkspaceCleanupScanError[]
}

export type WorkspaceCleanupScanProgress = WorkspaceCleanupScanResult & {
  scanId: string
  scannedWorktreeCount: number
  totalWorktreeCount: number
  candidateMode?: 'append' | 'snapshot'
}

export type WorkspaceCleanupLocalProcessResult = {
  hasKillableProcesses: boolean | null
}

export type WorkspaceCleanupDismissArgs = {
  dismissals: WorkspaceCleanupDismissal[]
}

export const WORKSPACE_CLEANUP_HARD_BLOCKERS: ReadonlySet<WorkspaceCleanupBlocker> = new Set([
  'main-worktree',
  'folder-repo',
  'pinned',
  'active-workspace',
  'running-terminal',
  'terminal-liveness-unknown',
  'dirty-editor-buffer',
  'volatile-local-context',
  'live-agent',
  'recent-visible-context',
  'ssh-disconnected',
  'git-status-error',
  'dirty-files',
  'unpushed-commits',
  'unknown-base',
  'dismissed'
])

export const WORKSPACE_CLEANUP_FORCE_REMOVE_BLOCKERS: ReadonlySet<WorkspaceCleanupBlocker> =
  new Set(['dirty-files', 'unpushed-commits', 'unknown-base', 'git-status-error'])

// Blockers that make a candidate unselectable in the cleanup UI (distinct from
// hard/force-remove blockers): structural cases the user can never queue.
const WORKSPACE_CLEANUP_QUEUE_BLOCKERS: ReadonlySet<WorkspaceCleanupBlocker> = new Set([
  'main-worktree',
  'folder-repo',
  'dismissed'
])

// UI-selectability predicate: a candidate can be queued for cleanup when it has
// at least one reason and no queue-blocking condition. Pure renderer-side helper
// (the classifier itself lives in the Rust workspace-cleanup core).
export function canQueueWorkspaceCleanupCandidate(
  candidate: Pick<WorkspaceCleanupCandidate, 'blockers' | 'reasons'>
): boolean {
  return (
    candidate.reasons.length > 0 &&
    !candidate.blockers.some((blocker) => WORKSPACE_CLEANUP_QUEUE_BLOCKERS.has(blocker))
  )
}
