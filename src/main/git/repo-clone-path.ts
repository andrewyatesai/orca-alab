import { resolve } from 'node:path'
import type { Stats } from 'node:fs'
import { lstat, mkdir, rm } from 'node:fs/promises'
import { isWindowsAbsolutePathLike } from '../../shared/cross-platform-path'
import { requireRustGitBinding } from '../daemon/rust-git-addon'

// The pure derivation/validation logic (repo name from URL, escape-safe clone
// path, WSL-UNC-aware comparison key) runs in the Rust `orca-git` core
// (repo_clone_path.rs — the TS bodies were deleted). Only the fs claim/cleanup
// IO and the cwd-dependent `resolve()` stay at this JS boundary.

export type ClaimedCloneTarget = {
  canCleanup: boolean
  ownedDirectoryIdentity: CloneDirectoryIdentity | null
}

type CloneDirectoryIdentity = Pick<Stats, 'dev' | 'ino' | 'birthtimeMs'>

export function deriveCloneRepoNameFromUrl(url: string): string {
  return requireRustGitBinding().deriveCloneRepoNameFromUrl(url)
}

export function deriveValidatedClonePath(args: { url: string; destination: string }): string {
  return requireRustGitBinding().deriveValidatedClonePath(
    args.url,
    args.destination,
    process.platform === 'win32' ? 'win32' : 'posix'
  )
}

export function getClonePathComparisonKey(clonePath: string): string {
  // Why: `resolve()` is cwd-dependent IO, so it stays in JS; Rust receives an
  // already-absolute path (Windows-absolute-like paths pass through unresolved,
  // matching the old TS behaviour on non-Windows hosts).
  const resolvedClonePath = isWindowsAbsolutePathLike(clonePath) ? clonePath : resolve(clonePath)
  return requireRustGitBinding().getClonePathComparisonKey(resolvedClonePath)
}

export async function claimCloneTarget(clonePath: string): Promise<ClaimedCloneTarget> {
  try {
    await mkdir(clonePath, { recursive: false })
    return {
      canCleanup: true,
      ownedDirectoryIdentity: cloneDirectoryIdentity(await lstat(clonePath))
    }
  } catch (error) {
    if (isErrnoCode(error, 'EEXIST')) {
      return { canCleanup: false, ownedDirectoryIdentity: null }
    }
    throw error
  }
}

export async function cleanupClaimedCloneTarget(
  clonePath: string,
  claimedTarget: ClaimedCloneTarget
): Promise<void> {
  if (!claimedTarget.canCleanup || !claimedTarget.ownedDirectoryIdentity) {
    return
  }

  try {
    const currentStats = await lstat(clonePath)
    if (!currentStats.isDirectory()) {
      return
    }
    if (
      !isSameCloneDirectoryIdentity(
        claimedTarget.ownedDirectoryIdentity,
        cloneDirectoryIdentity(currentStats)
      )
    ) {
      return
    }
  } catch {
    return
  }

  await rm(clonePath, { recursive: true, force: true }).catch(() => {
    // Best-effort cleanup - do not mask the original clone failure.
  })
}

function cloneDirectoryIdentity(stats: Stats): CloneDirectoryIdentity {
  // Why: fast remove/recreate cycles can reuse an inode; birthtime keeps us
  // from treating a replacement directory as the clone target we created.
  return { dev: stats.dev, ino: stats.ino, birthtimeMs: stats.birthtimeMs }
}

function isSameCloneDirectoryIdentity(
  a: CloneDirectoryIdentity,
  b: CloneDirectoryIdentity
): boolean {
  return a.dev === b.dev && a.ino === b.ino && a.birthtimeMs === b.birthtimeMs
}

function isErrnoCode(error: unknown, code: string): boolean {
  return error instanceof Error && (error as NodeJS.ErrnoException).code === code
}
