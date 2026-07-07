import { isNoUpstreamError, normalizeGitErrorMessage } from './rust-git-remote-error'
import { resolveEffectiveGitUpstream } from '../../shared/git-effective-upstream'
import { resolveGitRemoteRebaseSourceNative } from './rust-rebase-source'
import { gitPushNative } from './rust-push'
import type { GitPushTarget } from '../../shared/types'
import type { GitRuntimeOptions } from './git-runtime-options'
import { gitOptionsForWorktree } from './git-runtime-options'
import { validateGitPushTarget } from './push-target-validation'
import { gitExecFileAsync } from './runner'

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
  try {
    if (pushTarget) {
      const target = await validateGitPushTarget(worktreePath, pushTarget, options)
      await gitExecFileAsync(
        ['pull', ...pullArgs, target.remoteName, target.branchName],
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
        ['pull', ...pullArgs, upstream.remoteName, upstream.branchName],
        gitOptionsForWorktree(worktreePath, options)
      )
      return
    }

    await gitExecFileAsync(['pull', ...pullArgs], gitOptionsForWorktree(worktreePath, options))
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
  await gitPullWithArgs(worktreePath, [], pushTarget, options)
}

export async function gitFastForward(
  worktreePath: string,
  pushTarget?: GitPushTarget,
  options: GitRuntimeOptions = {}
): Promise<void> {
  await gitPullWithArgs(worktreePath, ['--ff-only'], pushTarget, options)
}

export async function gitPullRebaseFromBase(
  worktreePath: string,
  baseRef: string,
  options: GitRuntimeOptions = {}
): Promise<void> {
  try {
    // Rust resolves the remote/branch (read-only: git remote + check-ref-format);
    // runner.ts still executes the mutating `git pull --rebase` below.
    const source = await resolveGitRemoteRebaseSourceNative(
      (args) => gitExecFileAsync(args, gitOptionsForWorktree(worktreePath, options)),
      baseRef
    )
    await gitExecFileAsync(
      ['pull', '--rebase', source.remoteName, source.branchName],
      gitOptionsForWorktree(worktreePath, options)
    )
  } catch (error) {
    throw new Error(normalizeGitErrorMessage(error, 'pull'))
  }
}

export async function gitFetch(
  worktreePath: string,
  pushTarget?: GitPushTarget,
  options: GitRuntimeOptions = {}
): Promise<void> {
  try {
    if (pushTarget) {
      const target = await validateGitPushTarget(worktreePath, pushTarget, options)
      await gitExecFileAsync(
        ['fetch', '--prune', target.remoteName],
        gitOptionsForWorktree(worktreePath, options)
      )
      return
    }
    await gitExecFileAsync(['fetch', '--prune'], gitOptionsForWorktree(worktreePath, options))
  } catch (error) {
    throw new Error(normalizeGitErrorMessage(error, 'fetch'))
  }
}
