import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync
} from 'node:fs'
import {
  DAEMON_SESSIONS_DIR_NAME,
  HISTORY_DIR_MODE,
  HISTORY_FILE_MODE,
  __resetHistoryStoreLayoutForTests,
  getDaemonSessionStoreRoot,
  migrateLegacyDaemonSessionDirs,
  prepareDaemonSessionStoreRoot,
  tightenDaemonSessionStorePermissions
} from './history-store-layout'

const itOnPosix = process.platform === 'win32' ? it.skip : it

function fileMode(path: string): number {
  return statSync(path).mode & 0o777
}

describe('history store layout', () => {
  let terminalHistoryRoot: string

  beforeEach(() => {
    terminalHistoryRoot = mkdtempSync(join(tmpdir(), 'history-layout-'))
    __resetHistoryStoreLayoutForTests()
  })

  afterEach(() => {
    rmSync(terminalHistoryRoot, { recursive: true, force: true })
    __resetHistoryStoreLayoutForTests()
  })

  function makeLegacyDaemonDir(name: string, opts?: { corruptMeta?: boolean }): string {
    const dir = join(terminalHistoryRoot, encodeURIComponent(name))
    mkdirSync(dir, { recursive: true })
    if (opts?.corruptMeta) {
      writeFileSync(join(dir, 'meta.json'), '{nope')
    } else {
      writeFileSync(
        join(dir, 'meta.json'),
        JSON.stringify({
          cwd: '/home/user',
          cols: 80,
          rows: 24,
          startedAt: '2026-01-01T00:00:00.000Z',
          endedAt: null,
          exitCode: null
        })
      )
    }
    writeFileSync(join(dir, 'checkpoint.json'), '{"snapshotAnsi":"hello"}')
    return dir
  }

  function makeShellHistoryDir(hash: string): string {
    const dir = join(terminalHistoryRoot, hash)
    mkdirSync(dir, { recursive: true })
    writeFileSync(
      join(dir, 'meta.json'),
      JSON.stringify({ worktreeId: 'repo::/w', createdAt: '2026-01-01T00:00:00.000Z' })
    )
    writeFileSync(join(dir, 'zsh_history'), 'ls\n')
    return dir
  }

  describe('migrateLegacyDaemonSessionDirs', () => {
    it('moves daemon-shaped dirs into the subdir and leaves shell dirs alone', () => {
      const daemonDir = makeLegacyDaemonDir('wt::/a@@12345678')
      const shellDir = makeShellHistoryDir('a1b2c3d4e5f60718')
      const sessionsRoot = getDaemonSessionStoreRoot(terminalHistoryRoot)
      mkdirSync(sessionsRoot, { recursive: true })

      const moved = migrateLegacyDaemonSessionDirs(terminalHistoryRoot, sessionsRoot)

      expect(moved).toBe(1)
      expect(existsSync(daemonDir)).toBe(false)
      const migrated = join(sessionsRoot, encodeURIComponent('wt::/a@@12345678'))
      expect(existsSync(migrated)).toBe(true)
      expect(readFileSync(join(migrated, 'checkpoint.json'), 'utf-8')).toContain('hello')
      expect(existsSync(shellDir)).toBe(true)
    })

    it('migrates corrupt-meta dirs by their daemon artifact files', () => {
      const corrupt = makeLegacyDaemonDir('wt::/b@@87654321', { corruptMeta: true })
      const sessionsRoot = getDaemonSessionStoreRoot(terminalHistoryRoot)
      mkdirSync(sessionsRoot, { recursive: true })

      expect(migrateLegacyDaemonSessionDirs(terminalHistoryRoot, sessionsRoot)).toBe(1)
      expect(existsSync(corrupt)).toBe(false)
    })

    it('is idempotent and drops a stale source when the target already exists', () => {
      const sessionsRoot = getDaemonSessionStoreRoot(terminalHistoryRoot)
      mkdirSync(sessionsRoot, { recursive: true })
      const name = encodeURIComponent('wt::/c@@11112222')
      makeLegacyDaemonDir('wt::/c@@11112222')
      // A newer build already recreated the session in the new layout.
      mkdirSync(join(sessionsRoot, name))
      writeFileSync(join(sessionsRoot, name, 'checkpoint.json'), '{"snapshotAnsi":"newer"}')

      migrateLegacyDaemonSessionDirs(terminalHistoryRoot, sessionsRoot)

      expect(existsSync(join(terminalHistoryRoot, name))).toBe(false)
      expect(readFileSync(join(sessionsRoot, name, 'checkpoint.json'), 'utf-8')).toContain('newer')

      // Second run: nothing daemon-shaped remains at the top level.
      expect(migrateLegacyDaemonSessionDirs(terminalHistoryRoot, sessionsRoot)).toBe(0)
    })
  })

  describe('prepareDaemonSessionStoreRoot', () => {
    it('creates the subdir, migrates legacy dirs, and is stable across calls', () => {
      makeLegacyDaemonDir('wt::/d@@33334444')

      const first = prepareDaemonSessionStoreRoot(terminalHistoryRoot)
      expect(first).toBe(getDaemonSessionStoreRoot(terminalHistoryRoot))
      expect(existsSync(join(first, encodeURIComponent('wt::/d@@33334444')))).toBe(true)

      // Second call (same process) is a fast path returning the same root.
      expect(prepareDaemonSessionStoreRoot(terminalHistoryRoot)).toBe(first)
      expect(first.endsWith(DAEMON_SESSIONS_DIR_NAME)).toBe(true)
    })

    itOnPosix('creates the store dirs 0o700', () => {
      const root = prepareDaemonSessionStoreRoot(terminalHistoryRoot)
      expect(fileMode(root)).toBe(HISTORY_DIR_MODE)
      expect(fileMode(terminalHistoryRoot)).toBe(HISTORY_DIR_MODE)
    })
  })

  describe('tightenDaemonSessionStorePermissions', () => {
    itOnPosix('chmods pre-existing loose dirs and files', () => {
      const sessionsRoot = getDaemonSessionStoreRoot(terminalHistoryRoot)
      const sessionDir = join(sessionsRoot, encodeURIComponent('wt::/e@@55556666'))
      mkdirSync(sessionDir, { recursive: true })
      const checkpoint = join(sessionDir, 'checkpoint.json')
      writeFileSync(checkpoint, '{}')
      chmodSync(sessionsRoot, 0o755)
      chmodSync(sessionDir, 0o755)
      chmodSync(checkpoint, 0o644)

      tightenDaemonSessionStorePermissions(sessionsRoot)

      expect(fileMode(sessionsRoot)).toBe(HISTORY_DIR_MODE)
      expect(fileMode(sessionDir)).toBe(HISTORY_DIR_MODE)
      expect(fileMode(checkpoint)).toBe(HISTORY_FILE_MODE)
    })
  })
})
