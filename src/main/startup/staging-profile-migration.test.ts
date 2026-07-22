import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  KEYCHAIN_COPY_MARKER_FILE,
  NEW_PROFILE_DIR_NAME,
  OLD_PROFILE_DIR_NAME,
  copyStagingSafeStorageKeychainItem,
  decideStagingProfileMigration,
  migrateStagingProfile,
  oldStagingProfilePath,
  readProfileDirState
} from './staging-profile-migration'

describe('staging-profile-migration', () => {
  let appDataPath: string
  let newProfilePath: string
  let oldProfilePath: string

  beforeEach(() => {
    appDataPath = mkdtempSync(join(os.tmpdir(), 'orca-staging-migration-test-'))
    newProfilePath = join(appDataPath, NEW_PROFILE_DIR_NAME)
    oldProfilePath = join(appDataPath, OLD_PROFILE_DIR_NAME)
  })

  afterEach(() => {
    rmSync(appDataPath, { recursive: true, force: true })
  })

  function seedOldProfile(): void {
    mkdirSync(oldProfilePath)
    writeFileSync(join(oldProfilePath, 'config.json'), '{"repos":["a"]}')
    mkdirSync(join(oldProfilePath, 'ai-vault'))
    writeFileSync(join(oldProfilePath, 'ai-vault', 'secrets.bin'), 'encrypted')
  }

  describe('readProfileDirState', () => {
    it('reports missing, empty, and populated dirs', () => {
      expect(readProfileDirState(join(appDataPath, 'nope'))).toBe('missing')
      mkdirSync(newProfilePath)
      expect(readProfileDirState(newProfilePath)).toBe('empty')
      writeFileSync(join(newProfilePath, 'config.json'), '{}')
      expect(readProfileDirState(newProfilePath)).toBe('populated')
    })

    it('treats a dir holding only .DS_Store as empty', () => {
      mkdirSync(newProfilePath)
      writeFileSync(join(newProfilePath, '.DS_Store'), '')
      expect(readProfileDirState(newProfilePath)).toBe('empty')
    })

    it('treats a plain file at the path as populated so it is never replaced', () => {
      writeFileSync(newProfilePath, 'not a dir')
      expect(readProfileDirState(newProfilePath)).toBe('populated')
    })
  })

  describe('decideStagingProfileMigration', () => {
    const base = {
      isPackaged: true,
      userDataPath: join('/appdata', NEW_PROFILE_DIR_NAME),
      oldProfileState: 'populated' as const,
      newProfileState: 'missing' as const
    }

    it('skips unpackaged runs', () => {
      expect(decideStagingProfileMigration({ ...base, isPackaged: false })).toEqual({
        action: 'skip',
        reason: 'not-packaged'
      })
    })

    it('skips when userData is not the ALab profile dir (dev/E2E redirects, public identity)', () => {
      for (const dirName of ['Orca', 'orca-dev', 'orca-e2e-1234']) {
        expect(
          decideStagingProfileMigration({ ...base, userDataPath: join('/appdata', dirName) })
        ).toEqual({ action: 'skip', reason: 'user-data-not-alab-profile' })
      }
    })

    it('skips when the old profile is missing or empty', () => {
      expect(decideStagingProfileMigration({ ...base, oldProfileState: 'missing' })).toEqual({
        action: 'skip',
        reason: 'no-old-profile-data'
      })
      expect(decideStagingProfileMigration({ ...base, oldProfileState: 'empty' })).toEqual({
        action: 'skip',
        reason: 'no-old-profile-data'
      })
    })

    it('never renames over a populated new profile', () => {
      expect(decideStagingProfileMigration({ ...base, newProfileState: 'populated' })).toEqual({
        action: 'skip',
        reason: 'new-profile-populated'
      })
    })

    it('renames when the new profile is missing, without a pre-remove', () => {
      expect(decideStagingProfileMigration(base)).toEqual({
        action: 'rename',
        oldProfilePath: join('/appdata', OLD_PROFILE_DIR_NAME),
        newProfilePath: base.userDataPath,
        removeEmptyNewProfileDirFirst: false
      })
    })

    it('renames when the new profile is empty, removing the empty dir first', () => {
      expect(decideStagingProfileMigration({ ...base, newProfileState: 'empty' })).toMatchObject({
        action: 'rename',
        removeEmptyNewProfileDirFirst: true
      })
    })
  })

  describe('migrateStagingProfile', () => {
    it('moves the old profile into place when the new dir is missing', () => {
      seedOldProfile()
      const decision = migrateStagingProfile({
        isPackaged: true,
        userDataPath: newProfilePath,
        platform: 'linux',
        log: vi.fn(),
        warn: vi.fn()
      })
      expect(decision.action).toBe('rename')
      expect(existsSync(oldProfilePath)).toBe(false)
      expect(readFileSync(join(newProfilePath, 'config.json'), 'utf-8')).toBe('{"repos":["a"]}')
      expect(readFileSync(join(newProfilePath, 'ai-vault', 'secrets.bin'), 'utf-8')).toBe(
        'encrypted'
      )
    })

    it('moves the old profile over an empty new dir', () => {
      seedOldProfile()
      mkdirSync(newProfilePath)
      const decision = migrateStagingProfile({
        isPackaged: true,
        userDataPath: newProfilePath,
        platform: 'linux',
        log: vi.fn(),
        warn: vi.fn()
      })
      expect(decision.action).toBe('rename')
      expect(existsSync(oldProfilePath)).toBe(false)
      expect(readFileSync(join(newProfilePath, 'config.json'), 'utf-8')).toBe('{"repos":["a"]}')
    })

    it('leaves a populated new profile untouched', () => {
      seedOldProfile()
      mkdirSync(newProfilePath)
      writeFileSync(join(newProfilePath, 'config.json'), '{"repos":["b"]}')
      const decision = migrateStagingProfile({
        isPackaged: true,
        userDataPath: newProfilePath,
        platform: 'linux',
        log: vi.fn(),
        warn: vi.fn()
      })
      expect(decision).toEqual({ action: 'skip', reason: 'new-profile-populated' })
      expect(readFileSync(join(newProfilePath, 'config.json'), 'utf-8')).toBe('{"repos":["b"]}')
      expect(existsSync(oldProfilePath)).toBe(true)
    })

    it('does nothing on a fresh install with no old profile', () => {
      const decision = migrateStagingProfile({
        isPackaged: true,
        userDataPath: newProfilePath,
        platform: 'linux',
        log: vi.fn(),
        warn: vi.fn()
      })
      expect(decision).toEqual({ action: 'skip', reason: 'no-old-profile-data' })
      expect(existsSync(newProfilePath)).toBe(false)
    })

    it('copies the safeStorage Keychain item after a successful rename on darwin', () => {
      seedOldProfile()
      const calls: string[][] = []
      const execFileSyncFn = vi.fn((_file: string, args: string[]) => {
        calls.push(args)
        if (
          args[0] === 'find-generic-password' &&
          args.includes('Orca ALab Edition Safe Storage')
        ) {
          throw new Error('item not found')
        }
        return 'old-secret\n'
      })
      migrateStagingProfile({
        isPackaged: true,
        userDataPath: newProfilePath,
        platform: 'darwin',
        execFileSyncFn,
        appExecutablePath: '/Applications/Orca ALab Edition.app/Contents/MacOS/Orca ALab Edition',
        log: vi.fn(),
        warn: vi.fn()
      })
      const addCall = calls.find((args) => args[0] === 'add-generic-password')
      expect(addCall).toContain('Orca ALab Edition Safe Storage')
      expect(addCall).toContain('old-secret')
      const marker = JSON.parse(
        readFileSync(join(newProfilePath, KEYCHAIN_COPY_MARKER_FILE), 'utf-8')
      )
      expect(marker.outcome).toBe('copied')
    })
  })

  describe('copyStagingSafeStorageKeychainItem', () => {
    it('skips non-darwin platforms without touching the CLI', () => {
      const execFileSyncFn = vi.fn()
      expect(
        copyStagingSafeStorageKeychainItem({
          newProfilePath,
          platform: 'win32',
          execFileSyncFn
        })
      ).toBe('skipped-platform')
      expect(execFileSyncFn).not.toHaveBeenCalled()
    })

    it('attempts at most once: a marker from a failed attempt blocks retries', () => {
      mkdirSync(newProfilePath)
      const execFileSyncFn = vi.fn(() => {
        throw new Error('keychain locked')
      })
      const warn = vi.fn()
      expect(
        copyStagingSafeStorageKeychainItem({
          newProfilePath,
          platform: 'darwin',
          execFileSyncFn,
          warn
        })
      ).toBe('old-item-missing')
      expect(existsSync(join(newProfilePath, KEYCHAIN_COPY_MARKER_FILE))).toBe(true)
      expect(
        copyStagingSafeStorageKeychainItem({
          newProfilePath,
          platform: 'darwin',
          execFileSyncFn,
          warn
        })
      ).toBe('skipped-marker-present')
    })

    it('does not overwrite an existing new-name Keychain item', () => {
      mkdirSync(newProfilePath)
      // Every security call succeeds → the new item already exists.
      const execFileSyncFn = vi.fn(() => '')
      expect(
        copyStagingSafeStorageKeychainItem({
          newProfilePath,
          platform: 'darwin',
          execFileSyncFn
        })
      ).toBe('new-item-already-present')
      expect(execFileSyncFn).toHaveBeenCalledTimes(1)
    })

    it('reports failure but never throws when add-generic-password fails', () => {
      mkdirSync(newProfilePath)
      const execFileSyncFn = vi.fn((_file: string, args: string[]) => {
        if (args[0] === 'add-generic-password') {
          throw new Error('errSecDuplicateItem')
        }
        if (args.includes('Orca ALab Edition Safe Storage') && !args.includes('-w')) {
          throw new Error('item not found')
        }
        return 'old-secret\n'
      })
      const warn = vi.fn()
      expect(
        copyStagingSafeStorageKeychainItem({
          newProfilePath,
          platform: 'darwin',
          execFileSyncFn,
          warn
        })
      ).toBe('failed')
      expect(warn).toHaveBeenCalled()
      const marker = JSON.parse(
        readFileSync(join(newProfilePath, KEYCHAIN_COPY_MARKER_FILE), 'utf-8')
      )
      expect(marker.outcome).toBe('failed')
    })
  })

  it('oldStagingProfilePath resolves the sibling of the new profile', () => {
    expect(oldStagingProfilePath(newProfilePath)).toBe(oldProfilePath)
  })
})
