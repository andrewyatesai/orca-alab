import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mkdtempSync, rmSync } from 'node:fs'
import { join } from 'node:path'
import { tmpdir } from 'node:os'
import { DatabaseSync } from 'node:sqlite'
import type * as ChromiumCookieSnapshotModule from './chromium-cookie-snapshot'

const { userData, sessionFromPartitionMock, cookiesSetMock, snapshotSpy } = vi.hoisted(() => ({
  userData: { dir: '' },
  sessionFromPartitionMock: vi.fn(),
  cookiesSetMock: vi.fn(async () => {}),
  snapshotSpy: vi.fn()
}))

vi.mock('electron', () => ({
  app: { getPath: () => userData.dir },
  BrowserWindow: { fromWebContents: vi.fn() },
  dialog: { showOpenDialog: vi.fn() },
  session: { fromPartition: sessionFromPartitionMock }
}))

// Delegate to the real snapshot but observe the calls: the Firefox import must
// go through the temporal-consistency snapshot, not a bare copyFileSync.
vi.mock('./chromium-cookie-snapshot', async (importOriginal) => {
  const actual = await importOriginal<typeof ChromiumCookieSnapshotModule>()
  snapshotSpy.mockImplementation(actual.createChromiumCookieSnapshot)
  return { ...actual, createChromiumCookieSnapshot: snapshotSpy }
})

import { importCookiesFromBrowser, type DetectedBrowser } from './browser-cookie-import'

function firefoxBrowser(cookiesPath: string): DetectedBrowser {
  return {
    family: 'firefox',
    label: 'Firefox',
    cookiesPath,
    profiles: [{ name: 'default', directory: 'default' }],
    selectedProfile: 'default'
  }
}

function writeFirefoxCookiesDb(dir: string): string {
  const path = join(dir, 'cookies.sqlite')
  const db = new DatabaseSync(path)
  db.exec(
    `CREATE TABLE moz_cookies (
      id INTEGER PRIMARY KEY, name TEXT, value TEXT, host TEXT, path TEXT,
      expiry INTEGER, isSecure INTEGER, isHttpOnly INTEGER, sameSite INTEGER
    )`
  )
  db.prepare(
    'INSERT INTO moz_cookies (name, value, host, path, expiry, isSecure, isHttpOnly, sameSite) VALUES (?, ?, ?, ?, ?, ?, ?, ?)'
  ).run('sid', 'abc123', 'example.com', '/', 0, 1, 0, 0)
  db.close()
  return path
}

describe('importCookiesFromBrowser Firefox', () => {
  beforeEach(() => {
    userData.dir = mkdtempSync(join(tmpdir(), 'orca-ff-import-'))
    sessionFromPartitionMock.mockReset()
    cookiesSetMock.mockReset()
    cookiesSetMock.mockResolvedValue(undefined)
    snapshotSpy.mockClear()
    sessionFromPartitionMock.mockReturnValue({ cookies: { set: cookiesSetMock } })
  })

  afterEach(() => {
    rmSync(userData.dir, { recursive: true, force: true })
  })

  it('imports through the consistency-checked snapshot instead of a bare copy', async () => {
    const cookiesPath = writeFirefoxCookiesDb(userData.dir)

    const result = await importCookiesFromBrowser(firefoxBrowser(cookiesPath), 'persist:test')

    expect(result.ok).toBe(true)
    // The snapshot helper (before/after stat + retry) must be the copy path.
    expect(snapshotSpy).toHaveBeenCalledWith(cookiesPath)
    expect(cookiesSetMock).toHaveBeenCalledTimes(1)
  })

  it('does not log cookie diagnostics to the console unless debugging is enabled', async () => {
    const logSpy = vi.spyOn(console, 'log').mockImplementation(() => {})
    delete process.env.ORCA_COOKIE_IMPORT_DEBUG

    // A missing DB triggers a diag() line; it must not reach the console sink.
    await importCookiesFromBrowser(
      firefoxBrowser(join(userData.dir, 'missing.sqlite')),
      'persist:test'
    )
    expect(logSpy).not.toHaveBeenCalled()

    process.env.ORCA_COOKIE_IMPORT_DEBUG = '1'
    await importCookiesFromBrowser(
      firefoxBrowser(join(userData.dir, 'missing.sqlite')),
      'persist:test'
    )
    expect(logSpy).toHaveBeenCalled()

    delete process.env.ORCA_COOKIE_IMPORT_DEBUG
    logSpy.mockRestore()
  })
})
