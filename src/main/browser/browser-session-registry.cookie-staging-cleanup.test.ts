import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { tmpdir } from 'node:os'

const { userData, sessionFromPartitionMock } = vi.hoisted(() => ({
  userData: { dir: '' },
  sessionFromPartitionMock: vi.fn()
}))

vi.mock('electron', () => ({
  app: { getPath: () => userData.dir },
  session: { fromPartition: sessionFromPartitionMock },
  systemPreferences: {
    getMediaAccessStatus: vi.fn(() => 'granted'),
    askForMediaAccess: vi.fn(async () => true)
  }
}))
vi.mock('./browser-manager', () => ({
  browserManager: {
    notifyPermissionDenied: vi.fn(),
    handleGuestWillDownload: vi.fn(),
    installCertificateRequestGuard: vi.fn(),
    removeCertificateRequestGuard: vi.fn()
  }
}))

import { browserSessionRegistry } from './browser-session-registry'
import { ORCA_BROWSER_PARTITION } from '../../shared/constants'

function makeSession(): Record<string, ReturnType<typeof vi.fn>> {
  return {
    setPermissionRequestHandler: vi.fn(),
    setPermissionCheckHandler: vi.fn(),
    setDevicePermissionHandler: vi.fn(),
    setDisplayMediaRequestHandler: vi.fn(),
    on: vi.fn(),
    removeListener: vi.fn(),
    clearStorageData: vi.fn().mockResolvedValue(undefined),
    clearCache: vi.fn().mockResolvedValue(undefined)
  }
}

function stagingDir(): string {
  return join(userData.dir, 'cookie-import-staging')
}

// Mirrors the plaintext staged DB + its WAL/SHM sidecars written by cookie import.
function writeStaged(name: string): string {
  const path = join(stagingDir(), name)
  mkdirSync(dirname(path), { recursive: true })
  writeFileSync(path, 'PLAINTEXT-COOKIES')
  writeFileSync(`${path}-wal`, 'wal')
  writeFileSync(`${path}-shm`, 'shm')
  return path
}

describe('BrowserSessionRegistry cookie staging cleanup', () => {
  beforeEach(() => {
    userData.dir = mkdtempSync(join(tmpdir(), 'orca-reg-staging-'))
    sessionFromPartitionMock.mockReset()
    sessionFromPartitionMock.mockImplementation(() => makeSession())
  })

  afterEach(() => {
    rmSync(userData.dir, { recursive: true, force: true })
  })

  it('unlinks the staged plaintext cookie DB (and sidecars) when a profile is deleted', async () => {
    const profile = browserSessionRegistry.createProfile('isolated', 'Staged Delete')!
    const staged = writeStaged('Cookies-x-1-uuid')
    browserSessionRegistry.setPendingCookieImport(profile.partition, staged)
    expect(existsSync(staged)).toBe(true)

    await browserSessionRegistry.deleteProfile(profile.id)

    expect(existsSync(staged)).toBe(false)
    expect(existsSync(`${staged}-wal`)).toBe(false)
    expect(existsSync(`${staged}-shm`)).toBe(false)
  })

  it('unlinks the staged plaintext cookie DB when the default import is undone', async () => {
    const staged = writeStaged('Cookies-default-1-uuid')
    browserSessionRegistry.setPendingCookieImport(ORCA_BROWSER_PARTITION, staged)

    await browserSessionRegistry.clearDefaultSessionCookies()

    expect(existsSync(staged)).toBe(false)
    expect(existsSync(`${staged}-wal`)).toBe(false)
  })

  it('sweeps crash-orphaned staged DBs on startup but keeps referenced ones', () => {
    const orphan = writeStaged('Cookies-orphan-1-uuid')
    const referenced = writeStaged('Cookies-ref-1-uuid')
    browserSessionRegistry.setPendingCookieImport(ORCA_BROWSER_PARTITION, referenced)

    browserSessionRegistry.initializeBrowserSessionsFromPersistedState()

    expect(existsSync(orphan)).toBe(false)
    expect(existsSync(`${orphan}-wal`)).toBe(false)
    expect(existsSync(referenced)).toBe(true)
    expect(existsSync(`${referenced}-wal`)).toBe(true)
  })

  it('refuses to unlink a staged path outside the staging directory (tampered metadata)', async () => {
    const outside = join(userData.dir, 'not-staging', 'secret.db')
    mkdirSync(dirname(outside), { recursive: true })
    writeFileSync(outside, 'IMPORTANT')
    browserSessionRegistry.setPendingCookieImport(ORCA_BROWSER_PARTITION, outside)

    await browserSessionRegistry.clearDefaultSessionCookies()

    expect(existsSync(outside)).toBe(true)
  })
})
