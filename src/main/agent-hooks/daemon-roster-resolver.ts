// Why: `--resume`/`--continue` FORK the session id, so the forked id has no
// spawn-time binding. Claude's daemon records each fork's parent in
// <configDir>/daemon/roster.json (dispatch.launch.sessionId names the parent
// transcript), and the hook payload's transcript_path locates configDir — so
// lineage resolves without knowing the user's CLAUDE_CONFIG_DIR.
//
// This runs on the main-thread hook request path for every event of a
// still-unbound session, so it is bounded and memoized: the read is size-capped
// and symlink-rejected (fail-closed, like the transcript readers), and parsed
// results are cached per roster path with a short TTL so a session that never
// binds cannot re-stat/re-read the file on each hook event.
import { lstatSync, readFileSync } from 'node:fs'
import { basename, dirname, isAbsolute, join } from 'node:path'

// Why: roster.json is a small worker→session index, not an append log; cap the
// read so a crafted or corrupt file can neither block the event loop nor OOM.
const ROSTER_MAX_BYTES = 512 * 1024

// Why: bound disk I/O to at most once per TTL per roster path regardless of
// hook-event rate; short enough that a freshly written fork still binds within
// a couple of events.
const ROSTER_CACHE_TTL_MS = 2_000

// Why: one entry per active Claude config dir; generous headroom while still
// bounding a leaky caller.
const ROSTER_CACHE_MAX_ENTRIES = 64

type RosterCacheEntry = {
  expiresAtMs: number
  // child sessionId → parent sessionId; null when the roster is
  // missing/oversized/symlinked/malformed (negative cache — still fail open).
  parents: Map<string, string> | null
}

const rosterCache = new Map<string, RosterCacheEntry>()

/** Test-only: drop memoized roster reads between cases. */
export function clearDaemonRosterCache(): void {
  rosterCache.clear()
}

function buildParentMap(workers: Record<string, unknown>): Map<string, string> {
  const parents = new Map<string, string>()
  for (const worker of Object.values(workers)) {
    if (typeof worker !== 'object' || worker === null) {
      continue
    }
    const w = worker as Record<string, unknown>
    const childId = w.sessionId
    if (typeof childId !== 'string' || childId.length === 0) {
      continue
    }
    const launch = ((w.dispatch as Record<string, unknown> | undefined)?.launch ?? {}) as Record<
      string,
      unknown
    >
    const ref = launch.sessionId ?? launch.transcriptPath
    const base = typeof ref === 'string' ? basename(ref) : ''
    if (!base.endsWith('.jsonl')) {
      continue
    }
    const parent = base.slice(0, -'.jsonl'.length)
    // Why: a self-referential parent is not a fork edge — skip it.
    if (parent.length > 0 && parent !== childId) {
      parents.set(childId, parent)
    }
  }
  return parents
}

function loadRosterParents(rosterPath: string): Map<string, string> | null {
  let stats
  try {
    // Why: lstat (not stat) so a symlinked roster is rejected, not followed —
    // a hook payload must not redirect the read at an attacker-chosen target.
    stats = lstatSync(rosterPath)
  } catch {
    return null
  }
  if (stats.isSymbolicLink() || !stats.isFile() || stats.size > ROSTER_MAX_BYTES) {
    return null
  }
  try {
    const parsed = JSON.parse(readFileSync(rosterPath, 'utf8')) as Record<string, unknown>
    if (typeof parsed.workers !== 'object' || parsed.workers === null) {
      return null
    }
    return buildParentMap(parsed.workers as Record<string, unknown>)
  } catch {
    return null
  }
}

function readRosterParents(rosterPath: string, nowMs: number): Map<string, string> | null {
  const cached = rosterCache.get(rosterPath)
  if (cached && nowMs < cached.expiresAtMs) {
    return cached.parents
  }
  const parents = loadRosterParents(rosterPath)
  rosterCache.set(rosterPath, { expiresAtMs: nowMs + ROSTER_CACHE_TTL_MS, parents })
  while (rosterCache.size > ROSTER_CACHE_MAX_ENTRIES) {
    const oldest = rosterCache.keys().next().value
    if (oldest === undefined) {
      break
    }
    rosterCache.delete(oldest)
  }
  return parents
}

export function resolveClaudeForkParentSessionId(
  sessionId: string,
  transcriptPath: string | null,
  nowMs: number = Date.now()
): string | null {
  // Why: reject relative paths outright — only an absolute transcript locates a
  // real configDir, and a relative one could resolve against the cwd.
  if (!transcriptPath || !isAbsolute(transcriptPath)) {
    return null
  }
  const projectsDir = dirname(dirname(transcriptPath))
  if (basename(projectsDir) !== 'projects') {
    return null
  }
  const rosterPath = join(dirname(projectsDir), 'daemon', 'roster.json')
  const parents = readRosterParents(rosterPath, nowMs)
  return parents?.get(sessionId) ?? null
}
