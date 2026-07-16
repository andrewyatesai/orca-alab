import { lstat, readFile } from 'node:fs/promises'
import * as path from 'node:path'
import { DEFAULT_GIT_STATUS_LIMIT } from './git-status-limit'

export type GitLineStats = { added?: number; removed?: number }

/** Counts additions for an untracked file's raw bytes: `null` = binary (no count),
 *  `0` = empty, else the trailing-newline-aware line count. The implementation is the
 *  Rust `orca-git` core (`count_additions_in_buffer`) via napi; injected from the main
 *  process so this shared module stays platform-agnostic. */
export type UntrackedAdditionsCounter = (buffer: Buffer) => number | null

// Limits how many untracked files we read at once when counting their lines,
// so a worktree with thousands of new files cannot exhaust file descriptors.
const UNTRACKED_READ_CONCURRENCY = 8
// Keep status polling cheap: large untracked files are commonly generated
// assets, and reading them every poll can stall the source-control sidebar.
export const MAX_UNTRACKED_LINE_COUNT_BYTES = 2 * 1024 * 1024
// Why: cap how many untracked files get line-counted per poll. Only the
// 100-9,999-file band reaches this counting path — DEFAULT_GIT_STATUS_LIMIT
// (10,000) truncates huge repos before line-stats run — so a large un-gitignored
// generated/dependency dir in that middle band would otherwise lstat (and, on
// change, read) every file each poll, lagging the source-control sidebar and
// spiking main-process CPU during active builds. Above this many untracked
// files, skip counting for the poll: rows still render, just without a +N,
// exactly as the huge-repo didHitLimit path already does. A few hundred is well
// above any hand-authored new-file set, which is the case the count serves.
export const MAX_UNTRACKED_LINE_COUNT_FILES = 500
// Why: the cache must hold at least one full status scan's untracked set
// (capped at DEFAULT_GIT_STATUS_LIMIT entries). A smaller cache is worse than
// none: a sequential scan over more files than the cap evicts every entry
// before the next poll revisits it, so every poll re-reads every untracked
// file's contents (#8013). 2x leaves headroom for a second window polling a
// different worktree; entries are ~200 bytes, so worst case is a few MB.
const UNTRACKED_STATS_CACHE_MAX_ENTRIES = 2 * DEFAULT_GIT_STATUS_LIMIT

type CachedUntrackedStats = {
  size: number
  mtimeMs: number
  ctimeMs: number
  stats: GitLineStats
}

const untrackedStatsCache = new Map<string, CachedUntrackedStats>()

// The `git diff --numstat` parser (parseNumstat) was deleted here: it is now the
// Rust `orca_git::numstat` core, reached via napi in the main process and via wasm
// in the relay (src/relay/git-wasm.ts). This module keeps only the runner-agnostic
// untracked-counting orchestration below.

/** Shared lstat gate + stat-keyed cache for untracked-file counting, independent of
 *  which counter produces the content stats (Rust napi in main, git numstat on the
 *  relay): symlinks count as one added line, non-regular/oversized files get no count,
 *  and unchanged files reuse cached stats so status polling stays cheap. */
export async function countUntrackedFileWithCache(
  absolutePath: string,
  countContent: () => Promise<GitLineStats>
): Promise<GitLineStats> {
  try {
    const fileStat = await lstat(absolutePath)
    const cached = untrackedStatsCache.get(absolutePath)
    if (
      cached &&
      cached.size === fileStat.size &&
      cached.mtimeMs === fileStat.mtimeMs &&
      cached.ctimeMs === fileStat.ctimeMs
    ) {
      // Why: Map eviction below removes the oldest-inserted key; re-inserting
      // on hit makes that LRU instead of FIFO, so a hot worktree's entries
      // survive another worktree's scan sharing this cache.
      untrackedStatsCache.delete(absolutePath)
      untrackedStatsCache.set(absolutePath, cached)
      return cached.stats
    }
    if (fileStat.isSymbolicLink()) {
      return rememberUntrackedStats(absolutePath, fileStat, { added: 1 })
    }
    if (!fileStat.isFile() || fileStat.size > MAX_UNTRACKED_LINE_COUNT_BYTES) {
      return rememberUntrackedStats(absolutePath, fileStat, {})
    }
    return rememberUntrackedStats(absolutePath, fileStat, await countContent())
  } catch {
    return {}
  }
}

async function countFileAdditions(
  absolutePath: string,
  count: UntrackedAdditionsCounter
): Promise<GitLineStats> {
  return countUntrackedFileWithCache(absolutePath, async () => {
    const buffer = await readFile(absolutePath)
    // Rust `orca-git` core (count_additions_in_buffer) via napi: null = binary (no count),
    // 0 = empty, else the trailing-newline-aware line count. Parity-tested vs the former
    // TS byte-loop in orca-git-napi-parity.test.ts; the loop is deleted (single source).
    const added = count(buffer)
    return added === null ? {} : { added }
  })
}

function rememberUntrackedStats(
  absolutePath: string,
  fileStat: { size: number; mtimeMs: number; ctimeMs: number },
  stats: GitLineStats
): GitLineStats {
  // Why: delete-before-set keeps refreshed entries at the recent end of the
  // Map's insertion order, preserving the LRU eviction contract.
  untrackedStatsCache.delete(absolutePath)
  untrackedStatsCache.set(absolutePath, {
    size: fileStat.size,
    mtimeMs: fileStat.mtimeMs,
    ctimeMs: fileStat.ctimeMs,
    stats
  })
  if (untrackedStatsCache.size > UNTRACKED_STATS_CACHE_MAX_ENTRIES) {
    const oldestKey = untrackedStatsCache.keys().next().value
    if (oldestKey) {
      untrackedStatsCache.delete(oldestKey)
    }
  }
  return stats
}

// Untracked files have no git-tracked baseline, so `git diff` ignores them.
// We count their contents directly to show an additions magnitude.
function createGitLineStatsAbortError(): Error {
  const error = new Error('The operation was aborted.')
  error.name = 'AbortError'
  return error
}

export async function collectUntrackedAdditions(
  worktreePath: string,
  untrackedPaths: readonly string[],
  count?: UntrackedAdditionsCounter,
  signal?: AbortSignal
): Promise<Map<string, GitLineStats>> {
  // Why: an aborted refresh must reject (not resolve an empty map) even on the
  // no-counter path, so a cancelled scan cannot look like a completed result.
  if (signal?.aborted) {
    throw createGitLineStatsAbortError()
  }
  const result = new Map<string, GitLineStats>()
  // No counter (an unbuilt dev tree where the native addon isn't loadable) → skip
  // untracked line counting rather than reimplement the byte loop in JS. The count is
  // the only thing affected; staged/unstaged numstat still flow. The relay uses its own
  // git-numstat collector instead of this path.
  if (!count) {
    return result
  }
  // Over the cap: skip untracked line-counting for this poll before any lstat/
  // read (see MAX_UNTRACKED_LINE_COUNT_FILES). Leaves those rows without a +N.
  if (untrackedPaths.length > MAX_UNTRACKED_LINE_COUNT_FILES) {
    return result
  }
  for (let i = 0; i < untrackedPaths.length; i += UNTRACKED_READ_CONCURRENCY) {
    // Why: an aborted refresh must reject (not resolve partial counts) so a
    // cancelled scan cannot look like a completed status result, and so we
    // stop burning (possibly remote-host) file I/O after cancellation.
    if (signal?.aborted) {
      throw createGitLineStatsAbortError()
    }
    const chunk = untrackedPaths.slice(i, i + UNTRACKED_READ_CONCURRENCY)
    await Promise.all(
      chunk.map(async (relativePath) => {
        result.set(
          relativePath,
          await countFileAdditions(path.join(worktreePath, relativePath), count)
        )
      })
    )
  }
  if (signal?.aborted) {
    throw createGitLineStatsAbortError()
  }
  return result
}

export function applyLineStats(
  entry: { added?: number; removed?: number },
  stats: GitLineStats | undefined
): void {
  if (!stats) {
    return
  }
  if (stats.added !== undefined) {
    entry.added = stats.added
  }
  if (stats.removed !== undefined) {
    entry.removed = stats.removed
  }
}
