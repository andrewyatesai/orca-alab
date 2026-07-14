// Why: the ChecksPanel polls GitLab MR checks every 30–120s but consumes only
// pipeline jobs + comments. This module gives that poll the same two guards the
// GitHub checks path (store slice `fetchPRChecks`) relies on, so GitLab isn't
// re-hitting `glab` uncached on every cycle:
//   1. a module-level inflight-dedup Map — collapses concurrent/rapid re-render
//      calls for the same MR into a single subprocess fan-out (mirrors
//      `inflightChecksRequests`), and
//   2. a short-TTL in-memory cache — a 30s poll inside the 60s window serves the
//      last payload instead of re-fetching (mirrors `checksCache` +
//      `CHECKS_CACHE_TTL`; GitHub also gets `gh api --cache 60s`, which `glab`
//      lacks, so this cache is GitLab's stand-in).
// It routes through the lightweight `gl.mrChecks` path (head_pipeline jobs +
// discussions), never the full `gl.workItemDetails` MR-dialog bundle.
import type { GitLabMRChecks } from '../../../../shared/types'
import { callRuntimeRpc, getActiveRuntimeTarget } from '@/runtime/runtime-rpc-client'

// Mirror the GitHub checks slice constants: checks change often, so 60s; an
// empty/absent job list refreshes faster so a just-started pipeline surfaces.
const CHECKS_CACHE_TTL = 60_000
const EMPTY_CHECKS_CACHE_TTL = 10_000
// Bound the cache so a long session across many MRs can't grow unbounded.
const MAX_CACHE_ENTRIES = 100

type CacheEntry = { data: GitLabMRChecks; fetchedAt: number }

const checksCache = new Map<string, CacheEntry>()
const inflightRequests = new Map<string, Promise<GitLabMRChecks | null>>()

type FetchArgs = {
  repoPath: string
  repoId?: string
  settings: Parameters<typeof getActiveRuntimeTarget>[0]
  iid: number
  headSha?: string | null
  /** Bypass the TTL freshness check (still participates in inflight dedup). */
  force?: boolean
}

function cacheKey(args: FetchArgs): string {
  // Why: scope to the MR head commit so a new push invalidates stale checks;
  // repoId (falling back to path) isolates same-iid MRs across repos.
  return `${args.repoId ?? args.repoPath}::${args.iid}::${args.headSha ?? ''}`
}

function entryTtl(entry: CacheEntry): number {
  const jobs = entry.data.pipelineJobs
  return jobs === undefined || jobs.length === 0 ? EMPTY_CHECKS_CACHE_TTL : CHECKS_CACHE_TTL
}

function isFresh(entry: CacheEntry | undefined): entry is CacheEntry {
  return entry !== undefined && Date.now() - entry.fetchedAt < entryTtl(entry)
}

function storeEntry(key: string, data: GitLabMRChecks): void {
  // Re-insert to refresh recency (Map preserves insertion order → oldest first).
  checksCache.delete(key)
  checksCache.set(key, { data, fetchedAt: Date.now() })
  while (checksCache.size > MAX_CACHE_ENTRIES) {
    const oldest = checksCache.keys().next().value
    if (oldest === undefined) {
      break
    }
    checksCache.delete(oldest)
  }
}

async function requestMRChecks(args: FetchArgs): Promise<GitLabMRChecks | null> {
  const target = getActiveRuntimeTarget(args.settings)
  if (target.kind === 'environment') {
    return callRuntimeRpc<GitLabMRChecks | null>(
      target,
      'gitlab.mrChecks',
      {
        repo: args.repoId ?? args.repoPath,
        iid: args.iid
      },
      { timeoutMs: 30_000 }
    )
  }
  return (await window.api.gl.mrChecks({
    repoPath: args.repoPath,
    repoId: args.repoId,
    iid: args.iid
  })) as GitLabMRChecks | null
}

/**
 * Fetch the lightweight GitLab MR checks payload (pipeline jobs + comments) with
 * TTL caching and inflight de-duplication. The ChecksPanel poll calls this every
 * cycle; concurrent/burst calls collapse to one `glab` fan-out and a 30s poll
 * inside the 60s window serves cache instead of re-fetching.
 */
export async function fetchGitLabMRChecks(args: FetchArgs): Promise<GitLabMRChecks | null> {
  const key = cacheKey(args)
  const cached = checksCache.get(key)
  if (!args.force && isFresh(cached)) {
    return cached.data
  }

  const existing = inflightRequests.get(key)
  if (existing) {
    return existing
  }

  const request = (async () => {
    try {
      const result = await requestMRChecks(args)
      if (result) {
        storeEntry(key, result)
      }
      return result
    } finally {
      inflightRequests.delete(key)
    }
  })()
  inflightRequests.set(key, request)
  return request
}

/** @internal — test-only reset so cache/inflight state doesn't leak across cases. */
export function _resetGitLabMRChecksCacheForTest(): void {
  checksCache.clear()
  inflightRequests.clear()
}
