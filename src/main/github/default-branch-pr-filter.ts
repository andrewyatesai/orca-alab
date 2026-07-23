import type { PRState } from '../../shared/types'

/**
 * Strip the remote prefix from a default base ref (e.g. `origin/main` -> `main`,
 * `upstream/release/v2` -> `release/v2`) so it can be compared to a local branch name.
 * Why: `resolveDefaultBaseRefViaExec` returns the ref with its remote segment; local
 * checked-out branch names carry no remote prefix.
 */
export function defaultBaseRefToBranchName(defaultBaseRef: string | null): string | null {
  if (!defaultBaseRef) {
    return null
  }
  const slash = defaultBaseRef.indexOf('/')
  if (slash < 0) {
    return defaultBaseRef
  }
  const branch = defaultBaseRef.slice(slash + 1)
  return branch.length > 0 ? branch : null
}

/**
 * Whether a branch-discovered PR is a stale historical match that must not be surfaced.
 *
 * Why: on the repository default branch the REST lookup
 * `pulls?head=owner:<default>&state=all&per_page=1` returns the newest PR that ever had
 * head=<default> — including a long-closed one — and it wrongly appears in the Checks and
 * Source Control tabs (#9171). We only drop CLOSED implicit matches on the default branch:
 * a real OPEN PR from the default branch (fork master->upstream, master->release base) is
 * still shown, and MERGED matches keep their existing head-oid preservation logic.
 */
export function isStaleClosedDefaultBranchPR(args: {
  branchName: string
  defaultBranchName: string | null
  prState: PRState
  linkedPRNumber?: number | null
}): boolean {
  if (typeof args.linkedPRNumber === 'number') {
    return false
  }
  if (!args.defaultBranchName || args.branchName !== args.defaultBranchName) {
    return false
  }
  return args.prState === 'closed'
}
