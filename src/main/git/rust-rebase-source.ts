import type { GitRemoteRebaseSource } from '../../shared/git-rebase-source'
import { requireRustGitBinding } from '../daemon/rust-git-addon'
import { makeRustGitExecutor, type RunGit } from './rust-git-executor'

// Main-process rebase-source resolution runs through the verified Rust resolver
// via the napi addon — the sole path (the addon is required). Rust lists remotes
// and picks the longest match; `runGit` still executes git (SSH/WSL-safe). The
// resolver does NOT normalize its error; the caller (gitPullRebaseFromBase) keeps
// its outer normalizeGitErrorMessage(err, 'pull'). The shared TS resolver stays
// for the addon-less SSH relay + parity oracle — it is NOT a fallback here.

/**
 * Drop-in for the shared `resolveGitRemoteRebaseSource`: resolve a base ref to the
 * `remote`/`branch` pair `git pull --rebase` needs, driven in Rust. Rejects with the
 * raw resolver message (e.g. "Choose a remote base branch to rebase from.").
 */
export async function resolveGitRemoteRebaseSourceNative(
  runGit: RunGit,
  baseRef: string
): Promise<GitRemoteRebaseSource> {
  const json = await requireRustGitBinding().resolveGitRemoteRebaseSourceViaExecutor(
    baseRef,
    makeRustGitExecutor(runGit)
  )
  return JSON.parse(json) as GitRemoteRebaseSource
}
