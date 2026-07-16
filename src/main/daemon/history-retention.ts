import { join } from 'node:path'
import { readdirSync, readFileSync, rmSync, statSync } from 'node:fs'
import { PTY_SESSION_ID_SEPARATOR } from '../../shared/pty-session-id-format'
import { getDaemonSessionStoreRoot } from './history-store-layout'
import { getHistorySessionDirName } from './history-paths'
import { planSessionHistoryGc } from './daemon-session-history-gc-plan'

// Retention constants (documented; scrollback is secret-bearing, so every
// bound below is a privacy bound as much as a disk bound):
//
// ENDED_SESSION_RETENTION_MS — dirs whose meta.endedAt is stamped can no
// longer cold-restore (the reader rejects them); they exist only for the
// spawn-probe race window and quit-time stamping. A day is generous.
export const ENDED_SESSION_RETENTION_MS = 24 * 60 * 60 * 1000
// UNRESTORED_SESSION_RETENTION_MS — endedAt=null dirs are crash leftovers or
// slept sessions awaiting wake; they must survive "until restored or GC'd".
// Two weeks of never being touched means nobody is coming back for them.
export const UNRESTORED_SESSION_RETENTION_MS = 14 * 24 * 60 * 60 * 1000
// SESSION_STORE_MAX_TOTAL_BYTES — global cap; worst case a session dir is
// ~10MB (5MB log cap + multi-MB checkpoint), so this bounds the store to
// roughly 25-50 heavy sessions. Eviction is oldest-first by activity.
export const SESSION_STORE_MAX_TOTAL_BYTES = 256 * 1024 * 1024
// GC_MIN_DIR_AGE_MS — TOCTOU guard (same rationale as the shell-history GC):
// a dir created between the liveness snapshot and the scan must not be
// reaped, so anything touched in the last 10 minutes is off limits.
export const GC_MIN_DIR_AGE_MS = 10 * 60 * 1000

/**
 * Delete every daemon session dir owned by a removed worktree. Session ids
 * are minted as `${worktreeId}@@${shortUuid}` and dir names are
 * encodeURIComponent(sessionId), so ownership is a decoded-prefix test.
 *
 * Sweeps both the daemon-owned subdir and the legacy top level of the
 * terminal-history root, so worktree removal works even if the layout
 * migration has not run in this process yet.
 */
export function sweepWorktreeDaemonSessionHistory(
  terminalHistoryRoot: string,
  worktreeId: string
): number {
  const prefix = `${worktreeId}${PTY_SESSION_ID_SEPARATOR}`
  let removed = 0
  for (const root of [getDaemonSessionStoreRoot(terminalHistoryRoot), terminalHistoryRoot]) {
    let entries: { isDirectory(): boolean; name: string }[]
    try {
      entries = readdirSync(root, { withFileTypes: true })
    } catch {
      continue
    }
    for (const entry of entries) {
      if (!entry.isDirectory()) {
        continue
      }
      let sessionId: string
      try {
        sessionId = decodeURIComponent(entry.name)
      } catch {
        continue
      }
      if (!sessionId.startsWith(prefix)) {
        continue
      }
      try {
        rmSync(join(root, entry.name), { recursive: true, force: true })
        removed++
      } catch (err) {
        console.warn(
          `[history:retention] failed to sweep ${entry.name}: ${err instanceof Error ? err.message : String(err)}`
        )
      }
    }
  }
  if (removed > 0) {
    console.log(`[history:retention] swept ${removed} daemon session dir(s) for removed worktree`)
  }
  return removed
}

type ScannedSessionDir = {
  path: string
  name: string
  totalBytes: number
  /** Newest mtime across the dir's files — "last activity". */
  lastActivityMs: number
  /** null = meta missing/corrupt (treated like a not-ended dir). */
  endedAt: string | null
}

function scanSessionDir(root: string, name: string): ScannedSessionDir | null {
  const path = join(root, name)
  let totalBytes = 0
  let lastActivityMs = 0
  try {
    lastActivityMs = statSync(path).mtimeMs
    for (const file of readdirSync(path)) {
      const stat = statSync(join(path, file))
      totalBytes += stat.size
      lastActivityMs = Math.max(lastActivityMs, stat.mtimeMs)
    }
  } catch {
    return null
  }
  let endedAt: string | null = null
  try {
    const meta = JSON.parse(readFileSync(join(path, 'meta.json'), 'utf-8'))
    endedAt = typeof meta?.endedAt === 'string' ? meta.endedAt : null
  } catch {
    endedAt = null
  }
  return { path, name, totalBytes, lastActivityMs, endedAt }
}

export type SessionHistoryGcResult = {
  scanned: number
  expired: number
  evictedForSize: number
  remainingBytes: number
}

/**
 * Age + size garbage collection over the daemon session store.
 *
 * `liveSessionIds` is the authoritative liveness set (sessions currently
 * alive in any daemon); dirs for those ids are never touched. When liveness
 * is unknown (`null` — e.g. the daemon RPC failed), only provably-dead dirs
 * (stamped endedAt) are eligible, so a restorable crash dir can never be
 * lost to a flaky liveness probe.
 */
export function runDaemonSessionHistoryGc(
  sessionsRoot: string,
  opts: {
    liveSessionIds: Set<string> | null
    now?: number
    /** Test seam; production always uses SESSION_STORE_MAX_TOTAL_BYTES. */
    maxTotalBytes?: number
  }
): SessionHistoryGcResult {
  const now = opts.now ?? Date.now()
  const maxTotalBytes = opts.maxTotalBytes ?? SESSION_STORE_MAX_TOTAL_BYTES
  const result: SessionHistoryGcResult = {
    scanned: 0,
    expired: 0,
    evictedForSize: 0,
    remainingBytes: 0
  }
  let entries: { isDirectory(): boolean; name: string }[]
  try {
    entries = readdirSync(sessionsRoot, { withFileTypes: true })
  } catch {
    return result
  }
  const liveDirNames =
    opts.liveSessionIds === null
      ? null
      : new Set([...opts.liveSessionIds].map(getHistorySessionDirName))

  // Scan the store, then let the pure planner decide age-expiry + size eviction
  // (proven correct + TS↔Rust parity-checked in orca-session-gc); the fs work —
  // scanning and the rmSyncs, with their best-effort error handling — stays here.
  const scanned: ScannedSessionDir[] = []
  for (const entry of entries) {
    if (!entry.isDirectory()) {
      continue
    }
    const dir = scanSessionDir(sessionsRoot, entry.name)
    if (dir) {
      scanned.push(dir)
    }
  }
  result.scanned = scanned.length

  const plan = planSessionHistoryGc({
    dirs: scanned.map((dir) => ({
      name: dir.name,
      totalBytes: dir.totalBytes,
      lastActivityMs: dir.lastActivityMs,
      isEnded: dir.endedAt !== null
    })),
    now,
    maxTotalBytes,
    livenessUnknown: liveDirNames === null,
    liveDirNames,
    thresholds: {
      minDirAgeMs: GC_MIN_DIR_AGE_MS,
      endedRetentionMs: ENDED_SESSION_RETENTION_MS,
      unrestoredRetentionMs: UNRESTORED_SESSION_RETENTION_MS
    }
  })

  const pathByName = new Map(scanned.map((dir) => [dir.name, dir.path]))
  const deleted = new Set<string>()
  const applyDeletions = (names: string[], onSuccess: () => void): void => {
    for (const name of names) {
      const path = pathByName.get(name)
      if (path === undefined) {
        continue
      }
      try {
        rmSync(path, { recursive: true, force: true })
        deleted.add(name)
        onSuccess()
      } catch {
        // Best-effort: an undeletable dir still counts toward the size total and
        // is retried on the next GC pass (same as the pre-planner behavior).
      }
    }
  }
  applyDeletions(plan.expire, () => {
    result.expired++
  })
  applyDeletions(plan.evictForSize, () => {
    result.evictedForSize++
  })
  // Remaining = every scanned dir still on disk. On the happy path this equals the
  // plan's remainingBytes; on a failed rmSync the undeleted dir stays counted.
  result.remainingBytes = scanned.reduce(
    (sum, dir) => (deleted.has(dir.name) ? sum : sum + dir.totalBytes),
    0
  )
  console.log(
    `[history:retention:gc] scanned=${result.scanned} expired=${result.expired} evicted=${result.evictedForSize} remainingKB=${Math.ceil(result.remainingBytes / 1024)}`
  )
  return result
}

// Why 20s/6h: the startup pass waits out startup-critical I/O (the shell
// history GC uses 10s; stagger to avoid a same-tick disk pileup), and a
// periodic pass bounds growth for long-lived app sessions without waking
// the main process often.
const GC_STARTUP_DELAY_MS = 20_000
const GC_INTERVAL_MS = 6 * 60 * 60 * 1000

let gcScheduled = false

/**
 * Startup + periodic GC scheduling. `collectLiveSessionIds` resolves the
 * authoritative live set at run time (null = unknown); it is re-queried per
 * pass so daemon restarts and provider swaps are always reflected.
 */
export function scheduleDaemonSessionHistoryGc(opts: {
  getSessionsRoot: () => string
  collectLiveSessionIds: () => Promise<Set<string> | null>
}): void {
  if (gcScheduled) {
    return
  }
  gcScheduled = true
  const runPass = async (): Promise<void> => {
    try {
      const liveSessionIds = await opts.collectLiveSessionIds()
      runDaemonSessionHistoryGc(opts.getSessionsRoot(), { liveSessionIds })
    } catch (err) {
      console.warn(
        `[history:retention:gc] pass failed: ${err instanceof Error ? err.message : String(err)}`
      )
    }
  }
  setTimeout(() => void runPass(), GC_STARTUP_DELAY_MS).unref?.()
  setInterval(() => void runPass(), GC_INTERVAL_MS).unref?.()
}

export function __resetDaemonSessionHistoryGcScheduleForTests(): void {
  gcScheduled = false
}
