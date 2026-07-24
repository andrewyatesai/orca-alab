import { isNoUpstreamError, stripCredentialsFromMessage } from './rust-git-remote-error'
import {
  resolveEffectiveGitUpstream,
  type GitCommandRunner
} from '../../shared/git-effective-upstream'

/**
 * Git runner so branch-rename logic works identically for local worktrees
 * (`gitExecFileAsync`) and SSH worktrees (`provider.exec`). Same contract the
 * shared upstream-status helpers use.
 */
export type GitExec = GitCommandRunner

export type BranchUpstreamProbe =
  | { outcome: 'has-upstream' }
  | { outcome: 'no-upstream' }
  | { outcome: 'probe-failed'; message: string }

/**
 * Whether the branch has an upstream — i.e. it has been pushed or is tracking
 * a remote. Auto-rename refuses to touch such a branch because `git branch -m`
 * would orphan the remote branch and break any open PR.
 */
export async function probeBranchUpstream(exec: GitExec): Promise<BranchUpstreamProbe> {
  try {
    // Inject the Rust-backed no-upstream classifier: the shared TS predicate
    // was deleted, so this dual-bundled resolver takes it as a parameter.
    const upstream = await resolveEffectiveGitUpstream(exec, isNoUpstreamError)
    return { outcome: upstream !== null ? 'has-upstream' : 'no-upstream' }
  } catch (error) {
    if (isNoUpstreamError(error)) {
      return { outcome: 'no-upstream' }
    }
    // Why: an unexpected failure is not proof either way — report it as its own
    // outcome so callers skip the rename but stay retryable (issue #7808).
    // The message surfaces in the UI, so scrub credential-bearing remote URLs.
    return {
      outcome: 'probe-failed',
      message: stripCredentialsFromMessage(error instanceof Error ? error.message : String(error))
    }
  }
}

async function localBranchExists(exec: GitExec, branch: string): Promise<boolean> {
  try {
    await exec(['show-ref', '--verify', '--quiet', `refs/heads/${branch}`])
    return true
  } catch {
    return false
  }
}

/**
 * Resolve a branch name that doesn't collide with an existing local branch by
 * appending `-2`, `-3`, … to the leaf — the same suffixing worktree creation
 * uses. `compute` applies the configured prefix to a leaf. The branch currently
 * being renamed away from is never treated as a collision.
 */
export async function resolveUniqueBranchName(
  exec: GitExec,
  leaf: string,
  compute: (leaf: string) => string,
  currentBranch: string,
  maxAttempts = 100
): Promise<string | null> {
  const isAvailable = async (candidate: string): Promise<boolean> =>
    candidate === currentBranch || !(await localBranchExists(exec, candidate))

  const first = compute(leaf)
  if (await isAvailable(first)) {
    return first
  }
  for (let suffix = 2; suffix <= maxAttempts; suffix += 1) {
    const candidate = compute(`${leaf}-${suffix}`)
    if (await isAvailable(candidate)) {
      return candidate
    }
  }
  return null
}

async function readHead(exec: GitExec): Promise<string> {
  return (await exec(['rev-parse', '--abbrev-ref', 'HEAD'])).stdout.trim()
}

/**
 * Rename `currentBranch` to `newBranch`, but only while the user is still on it.
 * Why: two-arg `git branch -m <currentBranch> <newBranch>` renames by name and exits 0
 * even when currentBranch is NOT checked out, so a concurrent checkout in the same
 * worktree between validation and exec would let the rename settle and the caller
 * relabel a branch HEAD no longer points at (cxg2). Fail closed: refuse if HEAD already
 * moved, and — since HEAD only follows the rename when currentBranch was the checked-out
 * one — revert and throw if the post-rename HEAD isn't newBranch (a HEAD move that raced
 * the exec itself). No relabel unless the user is still on the branch being renamed.
 */
export async function renameCurrentBranch(
  exec: GitExec,
  currentBranch: string,
  newBranch: string
): Promise<void> {
  const headBefore = await readHead(exec)
  if (headBefore !== currentBranch) {
    throw new Error(
      `Refusing to rename "${currentBranch}": HEAD moved to "${headBefore}" before the rename.`
    )
  }
  await exec(['branch', '-m', currentBranch, newBranch])
  const headAfter = await readHead(exec)
  if (headAfter !== newBranch) {
    // HEAD didn't follow the rename -> currentBranch wasn't checked out at exec time.
    // Best-effort revert so the race leaves neither a stray rename nor a mislabel.
    try {
      await exec(['branch', '-m', newBranch, currentBranch])
    } catch {
      // Revert failed; the caller still fails closed below and re-validates on retry.
    }
    throw new Error(
      `Refusing to rename "${currentBranch}": HEAD moved to "${headAfter}" during the rename.`
    )
  }
}
