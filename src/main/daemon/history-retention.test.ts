import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { existsSync, mkdirSync, mkdtempSync, rmSync, utimesSync, writeFileSync } from 'node:fs'
import {
  ENDED_SESSION_RETENTION_MS,
  GC_MIN_DIR_AGE_MS,
  UNRESTORED_SESSION_RETENTION_MS,
  runDaemonSessionHistoryGc,
  sweepWorktreeDaemonSessionHistory
} from './history-retention'
import { DAEMON_SESSIONS_DIR_NAME, getDaemonSessionStoreRoot } from './history-store-layout'
import { getHistorySessionDirName } from './history-paths'

const WT_A = 'repo-1::/Users/dev/feature-a'
const WT_B = 'repo-1::/Users/dev/feature-b'

describe('history retention', () => {
  let terminalHistoryRoot: string
  let sessionsRoot: string

  beforeEach(() => {
    terminalHistoryRoot = mkdtempSync(join(tmpdir(), 'history-retention-'))
    sessionsRoot = getDaemonSessionStoreRoot(terminalHistoryRoot)
    mkdirSync(sessionsRoot, { recursive: true })
  })

  afterEach(() => {
    rmSync(terminalHistoryRoot, { recursive: true, force: true })
  })

  function makeSessionDir(
    root: string,
    sessionId: string,
    opts?: { endedAt?: string | null; ageMs?: number; bytes?: number; noMeta?: boolean }
  ): string {
    const dir = join(root, getHistorySessionDirName(sessionId))
    mkdirSync(dir, { recursive: true })
    if (!opts?.noMeta) {
      writeFileSync(
        join(dir, 'meta.json'),
        JSON.stringify({
          cwd: '/home/user',
          cols: 80,
          rows: 24,
          startedAt: '2026-01-01T00:00:00.000Z',
          endedAt: opts?.endedAt ?? null,
          exitCode: opts?.endedAt ? 0 : null
        })
      )
    }
    writeFileSync(join(dir, 'checkpoint.json'), 'x'.repeat(opts?.bytes ?? 16))
    if (opts?.ageMs !== undefined) {
      const t = new Date(Date.now() - opts.ageMs)
      // Backdate every file plus the dir itself — GC ages by newest mtime.
      utimesSync(join(dir, 'checkpoint.json'), t, t)
      if (!opts?.noMeta) {
        utimesSync(join(dir, 'meta.json'), t, t)
      }
      utimesSync(dir, t, t)
    }
    return dir
  }

  describe('sweepWorktreeDaemonSessionHistory', () => {
    it('removes only dirs whose decoded name has the worktree @@ prefix', () => {
      const a1 = makeSessionDir(sessionsRoot, `${WT_A}@@11111111`)
      const a2 = makeSessionDir(sessionsRoot, `${WT_A}@@22222222`)
      const b1 = makeSessionDir(sessionsRoot, `${WT_B}@@33333333`)
      // A shell-HISTFILE-style hash dir name must never match the prefix test.
      const shellDir = join(sessionsRoot, 'a1b2c3d4e5f60718')
      mkdirSync(shellDir)

      const removed = sweepWorktreeDaemonSessionHistory(terminalHistoryRoot, WT_A)

      expect(removed).toBe(2)
      expect(existsSync(a1)).toBe(false)
      expect(existsSync(a2)).toBe(false)
      expect(existsSync(b1)).toBe(true)
      expect(existsSync(shellDir)).toBe(true)
    })

    it('also sweeps pre-migration dirs at the legacy top level', () => {
      const legacy = makeSessionDir(terminalHistoryRoot, `${WT_A}@@99999999`)
      const kept = makeSessionDir(terminalHistoryRoot, `${WT_B}@@88888888`)

      const removed = sweepWorktreeDaemonSessionHistory(terminalHistoryRoot, WT_A)

      expect(removed).toBe(1)
      expect(existsSync(legacy)).toBe(false)
      expect(existsSync(kept)).toBe(true)
    })

    it('skips undecodable dir names and a worktree whose id is a prefix of another', () => {
      // '%' alone throws in decodeURIComponent — must be skipped, not crash.
      mkdirSync(join(sessionsRoot, 'not-percent-decodable-%zz'))
      const longer = makeSessionDir(sessionsRoot, `${WT_A}-longer@@44444444`)

      const removed = sweepWorktreeDaemonSessionHistory(terminalHistoryRoot, WT_A)

      expect(removed).toBe(0)
      expect(existsSync(longer)).toBe(true)
    })

    it('returns 0 when the store does not exist yet', () => {
      const emptyRoot = join(terminalHistoryRoot, 'nonexistent', DAEMON_SESSIONS_DIR_NAME)
      expect(sweepWorktreeDaemonSessionHistory(join(emptyRoot, '..'), WT_A)).toBe(0)
    })
  })

  describe('runDaemonSessionHistoryGc', () => {
    it('expires ended dirs past their retention, keeps recent ones', () => {
      const oldEnded = makeSessionDir(sessionsRoot, 'old-ended', {
        endedAt: '2026-01-01T01:00:00.000Z',
        ageMs: ENDED_SESSION_RETENTION_MS + 60_000
      })
      const freshEnded = makeSessionDir(sessionsRoot, 'fresh-ended', {
        endedAt: '2026-01-01T01:00:00.000Z',
        ageMs: GC_MIN_DIR_AGE_MS + 60_000
      })

      const result = runDaemonSessionHistoryGc(sessionsRoot, { liveSessionIds: new Set() })

      expect(result.expired).toBe(1)
      expect(existsSync(oldEnded)).toBe(false)
      expect(existsSync(freshEnded)).toBe(true)
    })

    it('never size-evicts dirs younger than the TOCTOU age guard', () => {
      // Two just-written dead dirs put the store over the cap, but both are
      // inside the age guard — the cap must wait rather than reap a dir that
      // may belong to a session spawned after the liveness snapshot.
      const youngA = makeSessionDir(sessionsRoot, 'young-a', {
        endedAt: '2026-01-01T01:00:00.000Z',
        bytes: 600
      })
      const youngB = makeSessionDir(sessionsRoot, 'young-b', {
        endedAt: '2026-01-01T01:00:00.000Z',
        bytes: 600
      })

      const result = runDaemonSessionHistoryGc(sessionsRoot, {
        liveSessionIds: new Set(),
        maxTotalBytes: 1000
      })

      expect(result.expired).toBe(0)
      expect(result.evictedForSize).toBe(0)
      expect(existsSync(youngA)).toBe(true)
      expect(existsSync(youngB)).toBe(true)
    })

    it('expires unrestored (endedAt=null) dirs only when liveness is known', () => {
      const crashLeftover = makeSessionDir(sessionsRoot, 'crash-leftover', {
        ageMs: UNRESTORED_SESSION_RETENTION_MS + 60_000
      })

      // Liveness unknown → fail-safe: a restorable dir is never expired.
      runDaemonSessionHistoryGc(sessionsRoot, { liveSessionIds: null })
      expect(existsSync(crashLeftover)).toBe(true)

      // Liveness known and the session is not alive → reclaimed.
      const result = runDaemonSessionHistoryGc(sessionsRoot, { liveSessionIds: new Set() })
      expect(result.expired).toBe(1)
      expect(existsSync(crashLeftover)).toBe(false)
    })

    it('never expires a live session dir, no matter how old', () => {
      const idleLive = makeSessionDir(sessionsRoot, 'idle-live', {
        ageMs: UNRESTORED_SESSION_RETENTION_MS * 3
      })

      const result = runDaemonSessionHistoryGc(sessionsRoot, {
        liveSessionIds: new Set(['idle-live'])
      })

      expect(result.expired).toBe(0)
      expect(existsSync(idleLive)).toBe(true)
    })

    it('evicts oldest-first over the size cap, sparing live dirs', () => {
      const bytes = 600
      const oldest = makeSessionDir(sessionsRoot, 'cap-oldest', {
        endedAt: '2026-01-01T01:00:00.000Z',
        ageMs: 3 * 60 * 60 * 1000,
        bytes
      })
      const middle = makeSessionDir(sessionsRoot, 'cap-middle', {
        endedAt: '2026-01-01T01:00:00.000Z',
        ageMs: 2 * 60 * 60 * 1000,
        bytes
      })
      const liveOld = makeSessionDir(sessionsRoot, 'cap-live', {
        ageMs: 4 * 60 * 60 * 1000,
        bytes
      })

      const result = runDaemonSessionHistoryGc(sessionsRoot, {
        liveSessionIds: new Set(['cap-live']),
        maxTotalBytes: 1500
      })

      // 3 dirs × 600B+meta against a 1500B cap → evictions must run; the
      // oldest dead dir goes first and the pass stops once under the cap.
      // The live dir, though the oldest overall, is untouchable.
      expect(result.evictedForSize).toBe(1)
      expect(existsSync(oldest)).toBe(false)
      expect(existsSync(middle)).toBe(true)
      expect(existsSync(liveOld)).toBe(true)
    })

    it('restricts size-cap eviction to ended dirs when liveness is unknown', () => {
      const bytes = 600
      const restorable = makeSessionDir(sessionsRoot, 'cap-restorable', {
        ageMs: 5 * 60 * 60 * 1000,
        bytes
      })
      const ended = makeSessionDir(sessionsRoot, 'cap-ended', {
        endedAt: '2026-01-01T01:00:00.000Z',
        ageMs: 2 * 60 * 60 * 1000,
        bytes
      })
      makeSessionDir(sessionsRoot, 'cap-fresh', { bytes })

      const result = runDaemonSessionHistoryGc(sessionsRoot, {
        liveSessionIds: null,
        maxTotalBytes: 1500
      })

      // The (older) restorable dir cannot be evicted without liveness — the
      // ended dir goes instead.
      expect(result.evictedForSize).toBe(1)
      expect(existsSync(restorable)).toBe(true)
      expect(existsSync(ended)).toBe(false)
    })

    it('treats meta-less dirs as unrestorable-unknown (kept without liveness, aged out with it)', () => {
      const junk = makeSessionDir(sessionsRoot, 'no-meta-junk', {
        noMeta: true,
        ageMs: UNRESTORED_SESSION_RETENTION_MS + 60_000
      })

      runDaemonSessionHistoryGc(sessionsRoot, { liveSessionIds: null })
      expect(existsSync(junk)).toBe(true)

      const result = runDaemonSessionHistoryGc(sessionsRoot, { liveSessionIds: new Set() })
      expect(result.expired).toBe(1)
      expect(existsSync(junk)).toBe(false)
    })

    it('returns zeroed stats for a missing store root', () => {
      const result = runDaemonSessionHistoryGc(join(terminalHistoryRoot, 'missing'), {
        liveSessionIds: new Set()
      })
      expect(result).toEqual({ scanned: 0, expired: 0, evictedForSize: 0, remainingBytes: 0 })
    })
  })
})
