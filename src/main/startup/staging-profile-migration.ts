import { execFileSync } from 'node:child_process'
import { existsSync, readdirSync, renameSync, rmSync, writeFileSync } from 'node:fs'
import { basename, dirname, join } from 'node:path'

/**
 * One-time adoption of the pre-rename 'Orca Staging' profile.
 *
 * Why: the productName rename 'Orca Staging' → 'Orca ALab Edition' (59574d931)
 * changed the default userData dir Electron derives from productName, so
 * updated installs would boot into an empty profile while the old one sits
 * orphaned next to it. On macOS the safeStorage Keychain item is also named
 * after productName, so without copying it the migrated profile's encrypted
 * secrets become undecryptable.
 */

export const OLD_PROFILE_DIR_NAME = 'Orca Staging'
export const NEW_PROFILE_DIR_NAME = 'Orca ALab Edition'
export const KEYCHAIN_COPY_MARKER_FILE = 'staging-keychain-copy.json'

const OLD_SAFE_STORAGE_SERVICE = 'Orca Staging Safe Storage'
const OLD_SAFE_STORAGE_ACCOUNT = 'Orca Staging'
const NEW_SAFE_STORAGE_SERVICE = 'Orca ALab Edition Safe Storage'
const NEW_SAFE_STORAGE_ACCOUNT = 'Orca ALab Edition'

export type ProfileDirState = 'missing' | 'empty' | 'populated'

export type StagingProfileMigrationDecision =
  | {
      action: 'rename'
      oldProfilePath: string
      newProfilePath: string
      removeEmptyNewProfileDirFirst: boolean
    }
  | {
      action: 'skip'
      reason:
        | 'not-packaged'
        | 'user-data-not-alab-profile'
        | 'no-old-profile-data'
        | 'new-profile-populated'
    }

export function oldStagingProfilePath(userDataPath: string): string {
  return join(dirname(userDataPath), OLD_PROFILE_DIR_NAME)
}

export function readProfileDirState(path: string): ProfileDirState {
  try {
    // Why: Finder drops .DS_Store into browsed dirs; it must not make a fresh profile look populated.
    const entries = readdirSync(path).filter((entry) => entry !== '.DS_Store')
    return entries.length === 0 ? 'empty' : 'populated'
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') {
      return 'missing'
    }
    // Why: a file at the path or an unreadable dir (ENOTDIR/EPERM) must never be replaced by a rename.
    return 'populated'
  }
}

export function decideStagingProfileMigration(input: {
  isPackaged: boolean
  userDataPath: string
  oldProfileState: ProfileDirState
  newProfileState: ProfileDirState
}): StagingProfileMigrationDecision {
  if (!input.isPackaged) {
    return { action: 'skip', reason: 'not-packaged' }
  }
  // Why: exact-name guard keeps dev/E2E redirects and public-identity ('Orca') builds from ever migrating.
  if (basename(input.userDataPath) !== NEW_PROFILE_DIR_NAME) {
    return { action: 'skip', reason: 'user-data-not-alab-profile' }
  }
  if (input.oldProfileState !== 'populated') {
    return { action: 'skip', reason: 'no-old-profile-data' }
  }
  if (input.newProfileState === 'populated') {
    return { action: 'skip', reason: 'new-profile-populated' }
  }
  return {
    action: 'rename',
    oldProfilePath: oldStagingProfilePath(input.userDataPath),
    newProfilePath: input.userDataPath,
    removeEmptyNewProfileDirFirst: input.newProfileState === 'empty'
  }
}

type ExecFileSyncFn = (file: string, args: string[]) => string

function runSecurityCli(file: string, args: string[]): string {
  return execFileSync(file, args, { encoding: 'utf-8', stdio: ['ignore', 'pipe', 'ignore'] })
}

export type KeychainCopyOutcome =
  | 'copied'
  | 'new-item-already-present'
  | 'old-item-missing'
  | 'failed'
  | 'skipped-marker-present'
  | 'skipped-platform'

export function copyStagingSafeStorageKeychainItem(options: {
  newProfilePath: string
  platform?: NodeJS.Platform
  execFileSyncFn?: ExecFileSyncFn
  appExecutablePath?: string
  warn?: (message: string) => void
}): KeychainCopyOutcome {
  const platform = options.platform ?? process.platform
  // Why: only macOS names its safeStorage backend after productName in a way the `security` CLI can copy; Linux keyring (libsecret) items may still need secrets re-entered.
  if (platform !== 'darwin') {
    return 'skipped-platform'
  }
  const markerPath = join(options.newProfilePath, KEYCHAIN_COPY_MARKER_FILE)
  if (existsSync(markerPath)) {
    return 'skipped-marker-present'
  }
  const execFileSyncFn = options.execFileSyncFn ?? runSecurityCli
  const warn = options.warn ?? ((message: string) => console.warn(message))
  let outcome: KeychainCopyOutcome = 'failed'
  try {
    let newItemExists = false
    try {
      execFileSyncFn('security', [
        'find-generic-password',
        '-s',
        NEW_SAFE_STORAGE_SERVICE,
        '-a',
        NEW_SAFE_STORAGE_ACCOUNT
      ])
      newItemExists = true
    } catch {
      // Missing item exits non-zero; that's the case we migrate.
    }
    if (newItemExists) {
      outcome = 'new-item-already-present'
    } else {
      let oldPassword: string | null = null
      try {
        oldPassword = execFileSyncFn('security', [
          'find-generic-password',
          '-s',
          OLD_SAFE_STORAGE_SERVICE,
          '-a',
          OLD_SAFE_STORAGE_ACCOUNT,
          '-w'
        ]).replace(/\n$/, '')
      } catch {
        outcome = 'old-item-missing'
      }
      if (oldPassword !== null) {
        const trustedAppArgs = options.appExecutablePath
          ? ['-T', options.appExecutablePath]
          : ['-T', process.execPath]
        execFileSyncFn('security', [
          'add-generic-password',
          '-s',
          NEW_SAFE_STORAGE_SERVICE,
          '-a',
          NEW_SAFE_STORAGE_ACCOUNT,
          '-w',
          oldPassword,
          ...trustedAppArgs
        ])
        outcome = 'copied'
      }
    }
  } catch (error) {
    outcome = 'failed'
    warn(`[staging-migration] safeStorage Keychain copy failed: ${String(error)}`)
  }
  try {
    // Why: the marker (not success) gates retries — a failing `security` call must never re-prompt on every launch.
    writeFileSync(
      markerPath,
      JSON.stringify({ schemeVersion: 1, attemptedAt: Date.now(), outcome })
    )
  } catch (error) {
    warn(`[staging-migration] could not write Keychain copy marker: ${String(error)}`)
  }
  return outcome
}

export function migrateStagingProfile(options: {
  isPackaged: boolean
  userDataPath: string
  platform?: NodeJS.Platform
  execFileSyncFn?: ExecFileSyncFn
  appExecutablePath?: string
  log?: (message: string) => void
  warn?: (message: string) => void
}): StagingProfileMigrationDecision {
  const log = options.log ?? ((message: string) => console.log(message))
  const warn = options.warn ?? ((message: string) => console.warn(message))
  const decision = decideStagingProfileMigration({
    isPackaged: options.isPackaged,
    userDataPath: options.userDataPath,
    oldProfileState: readProfileDirState(oldStagingProfilePath(options.userDataPath)),
    newProfileState: readProfileDirState(options.userDataPath)
  })
  if (decision.action !== 'rename') {
    return decision
  }
  try {
    if (decision.removeEmptyNewProfileDirFirst) {
      // Why: Windows renameSync cannot replace an existing dir; the dir was verified empty (.DS_Store aside).
      rmSync(decision.newProfilePath, { recursive: true, force: true })
    }
    // Why: renameSync (same appData volume) adopts the whole profile atomically — no copy, no partial state.
    renameSync(decision.oldProfilePath, decision.newProfilePath)
    log(
      `[staging-migration] adopted '${OLD_PROFILE_DIR_NAME}' profile as '${NEW_PROFILE_DIR_NAME}'`
    )
  } catch (error) {
    // Why: a failed migration must degrade to a fresh profile, never block startup.
    warn(`[staging-migration] profile rename failed: ${String(error)}`)
    return decision
  }
  copyStagingSafeStorageKeychainItem({
    newProfilePath: decision.newProfilePath,
    platform: options.platform,
    execFileSyncFn: options.execFileSyncFn,
    appExecutablePath: options.appExecutablePath,
    warn
  })
  return decision
}
