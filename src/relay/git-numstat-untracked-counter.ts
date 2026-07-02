/**
 * Untracked-file addition counts for the relay, computed by git itself.
 *
 * Why: the relay ships to SSH remote hosts without the per-arch native addon that
 * backs the Rust counter in the main process, and repo policy forbids a JS shadow
 * of that counter. `git diff --no-index --numstat /dev/null <file>` is git's own
 * canonical added-line count, so the +N badge stays consistent with local repos.
 */
import * as path from 'path'
import type { GitExec } from './git-handler-ops'
import {
  countUntrackedFileWithCache,
  type GitLineStats
} from '../shared/git-uncommitted-line-stats'

// Each count spawns a git process; bound the fan-out so a burst of new untracked
// files can't swamp the SSH host during a status refresh.
const GIT_NUMSTAT_CONCURRENCY = 4

function parseAddedCount(stdout: string): number | undefined {
  // Numstat line: "<added>\t<removed>\t<path>"; binary files report '-'.
  const added = stdout.split('\t', 1)[0]
  if (!added || added === '-') {
    return undefined
  }
  const count = Number.parseInt(added, 10)
  return Number.isFinite(count) && count >= 0 ? count : undefined
}

async function countFileViaGitNumstat(
  git: GitExec,
  worktreePath: string,
  absolutePath: string
): Promise<GitLineStats> {
  let stdout: string
  try {
    // The literal "/dev/null" is special-cased by git as the empty preimage even on
    // Windows, so this stays cross-platform without touching the filesystem.
    ;({ stdout } = await git(
      ['diff', '--no-index', '--numstat', '--', '/dev/null', absolutePath],
      worktreePath,
      { disableOptionalLocks: true }
    ))
  } catch (error) {
    const gitError = error as Error & { code?: number | string; stdout?: string }
    // Exit code 1 just means the paths differ — the normal case for untracked files.
    if (gitError.code !== 1 || typeof gitError.stdout !== 'string') {
      return {}
    }
    stdout = gitError.stdout
  }
  const added = parseAddedCount(stdout)
  return added === undefined ? {} : { added }
}

export async function collectUntrackedAdditionsViaGitNumstat(
  git: GitExec,
  worktreePath: string,
  untrackedPaths: readonly string[]
): Promise<Map<string, GitLineStats>> {
  const result = new Map<string, GitLineStats>()
  for (let i = 0; i < untrackedPaths.length; i += GIT_NUMSTAT_CONCURRENCY) {
    const chunk = untrackedPaths.slice(i, i + GIT_NUMSTAT_CONCURRENCY)
    await Promise.all(
      chunk.map(async (relativePath) => {
        // Absolute path: a file literally named "-" would otherwise make
        // `git diff --no-index` read stdin.
        const absolutePath = path.join(worktreePath, relativePath)
        result.set(
          relativePath,
          await countUntrackedFileWithCache(absolutePath, () =>
            countFileViaGitNumstat(git, worktreePath, absolutePath)
          )
        )
      })
    )
  }
  return result
}
