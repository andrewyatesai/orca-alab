import { execFileSync } from 'node:child_process'
import {
  existsSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  rmdirSync,
  writeFileSync
} from 'node:fs'
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

// Why: non-recursive on purpose — a racing second instance may have adopted the profile between
// the state read and here (TOCTOU); rmdir then ENOTEMPTYs into the rename-failure degrade path
// instead of recursively deleting the just-migrated profile.
export function removeVerifiedEmptyProfileDir(path: string): void {
  rmSync(join(path, '.DS_Store'), { force: true })
  rmdirSync(path)
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

export const MAX_KEYCHAIN_COPY_ATTEMPTS = 3

type KeychainCopyMarkerState = { blocked: boolean; priorAttempts: number }

function readKeychainCopyMarkerState(markerPath: string): KeychainCopyMarkerState {
  if (!existsSync(markerPath)) {
    return { blocked: false, priorAttempts: 0 }
  }
  try {
    const marker = JSON.parse(readFileSync(markerPath, 'utf-8')) as {
      outcome?: unknown
      attempts?: unknown
    }
    if (marker.outcome === 'copied' || marker.outcome === 'new-item-already-present') {
      return { blocked: true, priorAttempts: MAX_KEYCHAIN_COPY_ATTEMPTS }
    }
    // Why: schemeVersion-1 markers had no attempts field; count one attempt so installs wedged by a single transient failure retry.
    const priorAttempts = typeof marker.attempts === 'number' ? marker.attempts : 1
    return { blocked: priorAttempts >= MAX_KEYCHAIN_COPY_ATTEMPTS, priorAttempts }
  } catch {
    // Why: an unreadable marker fails closed — never risk a Keychain prompt on every launch.
    return { blocked: true, priorAttempts: MAX_KEYCHAIN_COPY_ATTEMPTS }
  }
}

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
  const markerState = readKeychainCopyMarkerState(markerPath)
  if (markerState.blocked) {
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
    // Why: terminal outcomes block retries for good; transient ones ('failed'/'old-item-missing') retry up to MAX so one locked-keychain launch cannot orphan safeStorage data.
    writeFileSync(
      markerPath,
      JSON.stringify({
        schemeVersion: 2,
        attemptedAt: Date.now(),
        outcome,
        attempts: markerState.priorAttempts + 1
      })
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
  if (decision.action === 'rename') {
    try {
      if (decision.removeEmptyNewProfileDirFirst) {
        // Why: Windows renameSync cannot replace an existing dir; the dir was verified empty (.DS_Store aside).
        removeVerifiedEmptyProfileDir(decision.newProfilePath)
      }
      // Why: renameSync (same appData volume) adopts the whole profile atomically — no copy, no partial state.
      renameSync(decision.oldProfilePath, decision.newProfilePath)
      log(
        `[staging-migration] adopted '${OLD_PROFILE_DIR_NAME}' profile as '${NEW_PROFILE_DIR_NAME}'`
      )
    } catch (error) {
      // Why: a failed migration must degrade to a fresh profile, never block startup.
      warn(`[staging-migration] profile rename failed: ${String(error)}`)
    }
  }
  // Why: not gated on this launch's rename — a crash after renameSync but before the marker write
  // must still get the Keychain copy next launch; item probes + marker keep it idempotent.
  if (
    options.isPackaged &&
    basename(options.userDataPath) === NEW_PROFILE_DIR_NAME &&
    existsSync(options.userDataPath)
  ) {
    copyStagingSafeStorageKeychainItem({
      newProfilePath: options.userDataPath,
      platform: options.platform,
      execFileSyncFn: options.execFileSyncFn,
      appExecutablePath: options.appExecutablePath,
      warn
    })
  }
  return decision
}
