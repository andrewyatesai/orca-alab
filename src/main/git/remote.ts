import { isNoUpstreamError, normalizeGitErrorMessage } from './rust-git-remote-error'
import { runPullWithDivergenceFallback } from '../../shared/git-remote-error'
import { resolveEffectiveGitUpstream } from '../../shared/git-effective-upstream'
import { gitPullRebaseFromBaseNative } from './rust-pull-rebase'
import { gitPushNative } from './rust-push'
import { gitFetchNative } from './rust-fetch'
import type { GitPushTarget } from '../../shared/types'
import type { GitRuntimeOptions } from './git-runtime-options'
import { gitOptionsForWorktree } from './git-runtime-options'
import { validateGitPushTarget } from './push-target-validation'
import { gitExecFileAsync } from './runner'
import { runWithGitReadCacheInvalidation } from './status'

export async function gitPush(
  worktreePath: string,
  _publish = false,
  pushTarget?: GitPushTarget,
  options: { forceWithLease?: boolean } & GitRuntimeOptions = {}
): Promise<void> {
  try {
    // Why: Rust drives the whole push — validate an explicit target, resolve the
    // refspec (explicit; else the branch's configured push remote so a PR-created
    // worktree tracking a contributor fork doesn't send review commits to upstream;
    // else first-publish origin/HEAD), and run the push. runner.ts still executes
    // the mutating `git push`, so SSH/WSL/env routing is preserved.
    await gitPushNative(
      (args) => gitExecFileAsync(args, gitOptionsForWorktree(worktreePath, options)),
      pushTarget,
      options.forceWithLease ?? false
    )
  } catch (error) {
    throw new Error(normalizeGitErrorMessage(error, 'push'))
  }
}

async function gitPullWithArgs(
  worktreePath: string,
  pullArgs: string[],
  pushTarget?: GitPushTarget,
  options: GitRuntimeOptions = {}
): Promise<void> {
  const runPull = async (effectiveArgs: string[]): Promise<void> => {
    if (pushTarget) {
      const target = await validateGitPushTarget(worktreePath, pushTarget, options)
      await gitExecFileAsync(
        ['pull', ...effectiveArgs, target.remoteName, target.branchName],
        gitOptionsForWorktree(worktreePath, options)
      )
      return
    }
    const upstream = await resolveEffectiveGitUpstream(
      (args) => gitExecFileAsync(args, gitOptionsForWorktree(worktreePath, options)),
      isNoUpstreamError
    )
    if (upstream && !upstream.isConfiguredUpstream) {
      // Why: legacy Orca branches may still track origin/main while pushes
      // target origin/<branch>. Pull the same effective branch the UI reports.
      await gitExecFileAsync(
        ['pull', ...effectiveArgs, upstream.remoteName, upstream.branchName],
        gitOptionsForWorktree(worktreePath, options)
      )
      return
    }

    await gitExecFileAsync(['pull', ...effectiveArgs], gitOptionsForWorktree(worktreePath, options))
  }

  try {
    await runPullWithDivergenceFallback(pullArgs, runPull)
  } catch (error) {
    throw new Error(normalizeGitErrorMessage(error, 'pull'))
  }
}

export async function gitPull(
  worktreePath: string,
  pushTarget?: GitPushTarget,
  options: GitRuntimeOptions = {}
): Promise<void> {
  // Why: plain `git pull` uses the user's configured pull strategy (merge by
  // default) so diverged branches reconcile instead of erroring out. Conflicts
  // surface through the existing conflict-resolution flow.
  await runWithGitReadCacheInvalidation(() =>
    gitPullWithArgs(worktreePath, [], pushTarget, options)
  )
}

export async function gitFastForward(
  worktreePath: string,
  pushTarget?: GitPushTarget,
  options: GitRuntimeOptions = {}
): Promise<void> {
  await runWithGitReadCacheInvalidation(() =>
    gitPullWithArgs(worktreePath, ['--ff-only'], pushTarget, options)
  )
}

export async function gitPullRebaseFromBase(
  worktreePath: string,
  baseRef: string,
  options: GitRuntimeOptions = {}
): Promise<void> {
  await runWithGitReadCacheInvalidation(async () => {
    try {
      // Why: Rust drives the whole rebase-from-base — resolve the base's
      // remote/branch (read-only: git remote + check-ref-format) AND run the
      // mutating `pull --rebase` — in one call; runner.ts still executes git, so
      // SSH/WSL/env routing is preserved.
      await gitPullRebaseFromBaseNative(
        (args) => gitExecFileAsync(args, gitOptionsForWorktree(worktreePath, options)),
        baseRef
      )
    } catch (error) {
      throw new Error(normalizeGitErrorMessage(error, 'pull'))
    }
  })
}

export async function gitFetch(
  worktreePath: string,
  pushTarget?: GitPushTarget,
  options: GitRuntimeOptions = {}
): Promise<void> {
  try {
    // Why: Rust drives the whole fetch — validate an explicit target, then run
    // `fetch --prune [<remote>]` — normalizing errors internally; runner.ts still
    // executes the mutating fetch, so SSH/WSL/env routing is preserved.
    await gitFetchNative(
      (args) => gitExecFileAsync(args, gitOptionsForWorktree(worktreePath, options)),
      pushTarget
    )
  } catch (error) {
    throw new Error(normalizeGitErrorMessage(error, 'fetch'))
  }
}
