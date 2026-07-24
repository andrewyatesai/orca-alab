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
import {
  DEFAULT_LOCAL_ORCA_PROFILE_ID,
  getOrcaProfileBrowserDefaultPartition,
  getOrcaProfileBrowserPartitionSegment
} from '../../shared/orca-profiles'

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

// Why: staging is namespaced per Orca profile under the shared root.
function stagingDir(orcaProfileId = DEFAULT_LOCAL_ORCA_PROFILE_ID): string {
  return join(
    userData.dir,
    'cookie-import-staging',
    getOrcaProfileBrowserPartitionSegment(orcaProfileId)
  )
}

// Mirrors the plaintext staged DB + its WAL/SHM sidecars written by cookie import.
function writeStaged(name: string, orcaProfileId = DEFAULT_LOCAL_ORCA_PROFILE_ID): string {
  const path = join(stagingDir(orcaProfileId), name)
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
    // Why: the registry is a module singleton; reset it to the default profile so
    // a prior test's configureForOrcaProfile can't leak the active profile.
    browserSessionRegistry.configureForOrcaProfile({
      orcaProfileId: DEFAULT_LOCAL_ORCA_PROFILE_ID,
      profileDirectory: userData.dir
    })
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

  // cxb2: a well-formed JSON meta whose pendingCookieImports holds a NON-STRING
  // value must not throw out of app init (it runs outside any try/catch).
  it('does not throw when persisted pendingCookieImports holds a non-string value', () => {
    const metaPath = join(userData.dir, 'browser-session-meta.json')
    writeFileSync(
      metaPath,
      JSON.stringify({
        pendingCookieImports: { [ORCA_BROWSER_PARTITION]: 12345, tampered: { nested: true } }
      })
    )

    expect(() => browserSessionRegistry.initializeBrowserSessionsFromPersistedState()).not.toThrow()
  })

  // cxb3: the staging root is shared across Orca profiles, but the sweep runs with
  // only the ACTIVE profile's references. Switching profiles must not delete
  // another profile's still-pending staged (decrypted) cookie DB.
  it("does not sweep another Orca profile's pending staged cookie DB", () => {
    const profileA = 'orca-profile-alpha'
    const profileB = 'orca-profile-beta'
    const dirA = join(userData.dir, 'profiles', profileA)
    const dirB = join(userData.dir, 'profiles', profileB)
    mkdirSync(dirA, { recursive: true })
    mkdirSync(dirB, { recursive: true })

    // Profile A has a pending staged import (pre-fix writer used the flat root).
    const stagedA = join(userData.dir, 'cookie-import-staging', 'Cookies-alpha-1-uuid')
    mkdirSync(dirname(stagedA), { recursive: true })
    writeFileSync(stagedA, 'PLAINTEXT-COOKIES-A')
    browserSessionRegistry.configureForOrcaProfile({
      orcaProfileId: profileA,
      profileDirectory: dirA
    })
    browserSessionRegistry.setPendingCookieImport(
      getOrcaProfileBrowserDefaultPartition(profileA),
      stagedA
    )
    expect(existsSync(stagedA)).toBe(true)

    // User switches to profile B; its startup sweep sees only B's (empty) references.
    browserSessionRegistry.configureForOrcaProfile({
      orcaProfileId: profileB,
      profileDirectory: dirB
    })
    browserSessionRegistry.initializeBrowserSessionsFromPersistedState()

    // A's still-pending staged DB must survive B's sweep.
    expect(existsSync(stagedA)).toBe(true)
  })
})
