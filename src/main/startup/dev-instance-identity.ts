import { createHash } from 'node:crypto'
import path from 'node:path'
// Type-only: erased at runtime, so importing it keeps this module loadable in
// non-Electron unit tests while typing the guarded lazy require below.
import type * as ElectronModule from 'electron'
import type { AppIdentity } from '../../shared/app-identity'

const BASE_APP_NAME = 'Orca'
const DEV_EDITION_APP_NAME = 'Orca: ALab Edition'
const BASE_APP_USER_MODEL_ID = 'com.stablyai.orca'
// Why: fork staging builds need a distinct AUMID so Windows taskbar grouping,
// notifications, and shortcuts never collide with an installed public Orca
// (staging-launch audit F14). Must equal the staging appId in
// config/electron-builder.config.cjs.
const FORK_APP_USER_MODEL_ID = 'com.stablyai.orca.staging'
// Raw package.json "name" — what Electron reports when no productName was
// injected into the packaged metadata (i.e. an upstream-identity build).
const RAW_PACKAGE_NAME = 'orca'
const MAX_LABEL_LENGTH = 80

export type DevInstanceIdentity = AppIdentity & {
  appUserModelId: string
  // Why: drives app.setName → the macOS safeStorage Keychain item name
  // ("<appName> Safe Storage"). Kept stable across dev branches (unlike the
  // per-branch `name`) so every dev instance shares one Keychain key instead of
  // creating a new one per branch and re-prompting. Distinct from prod's 'Orca'.
  appName: string
}

function cleanEnvValue(value: string | undefined): string | null {
  const trimmed = value?.replace(/\s+/g, ' ').trim()
  if (!trimmed) {
    return null
  }
  return trimmed.length > MAX_LABEL_LENGTH
    ? `${trimmed.slice(0, MAX_LABEL_LENGTH - 3)}...`
    : trimmed
}

function lastPathSegment(value: string): string {
  const normalized = value.replace(/\\/g, '/')
  return normalized.split('/').findLast(Boolean) ?? value
}

function formatLabel(branch: string | null, worktreeName: string | null): string | null {
  if (branch && worktreeName) {
    if (branch === worktreeName || lastPathSegment(branch) === worktreeName) {
      return worktreeName
    }
    return `${worktreeName} @ ${branch}`
  }
  return branch ?? worktreeName
}

function createDevAppUserModelId(identityKey: string | null): string {
  if (!identityKey) {
    return BASE_APP_USER_MODEL_ID
  }
  const hash = createHash('sha1').update(identityKey).digest('hex').slice(0, 10)
  return `${BASE_APP_USER_MODEL_ID}.dev.${hash}`
}

function readPackagedProductName(): string | null {
  // Why: fork staging builds inject their productName ("Orca Staging") into
  // the packaged package.json via electron-builder extraMetadata, and Electron
  // initializes app.name from it before any of this code runs. A guarded lazy
  // require keeps this module importable in non-Electron unit tests.
  if (!process.versions.electron || typeof require !== 'function') {
    return null
  }
  try {
    const { app } = require('electron') as typeof ElectronModule
    return app?.name ?? null
  } catch {
    return null
  }
}

export function getDevInstanceIdentity(
  isDev: boolean,
  env: NodeJS.ProcessEnv = process.env,
  packagedProductName: string | null = readPackagedProductName()
): DevInstanceIdentity {
  if (!isDev) {
    // Why: an injected productName that differs from upstream's names marks a
    // fork-identity build; echoing it back (instead of forcing 'Orca') keeps
    // app.setName from flipping the userData dir public Orca uses (audit F14).
    const forkProductName =
      packagedProductName &&
      packagedProductName !== RAW_PACKAGE_NAME &&
      packagedProductName !== BASE_APP_NAME
        ? packagedProductName
        : null
    return {
      name: forkProductName ?? BASE_APP_NAME,
      // Why: appName drives app.setName; echo the fork productName so a staging
      // build's Keychain item and userData dir never collapse into public Orca's.
      appName: forkProductName ?? BASE_APP_NAME,
      isDev: false,
      devLabel: null,
      devBranch: null,
      devWorktreeName: null,
      devRepoRoot: null,
      dockBadgeLabel: null,
      appUserModelId: forkProductName ? FORK_APP_USER_MODEL_ID : BASE_APP_USER_MODEL_ID
    }
  }

  const repoRoot = cleanEnvValue(env.ORCA_DEV_REPO_ROOT)
  const branch = cleanEnvValue(env.ORCA_DEV_BRANCH)
  const worktreeName =
    cleanEnvValue(env.ORCA_DEV_WORKTREE_NAME) ??
    cleanEnvValue(path.basename(repoRoot ?? process.cwd()))
  const devLabel = cleanEnvValue(env.ORCA_DEV_INSTANCE_LABEL) ?? formatLabel(branch, worktreeName)
  const dockTitle = cleanEnvValue(env.ORCA_DEV_DOCK_TITLE) ?? DEV_EDITION_APP_NAME

  return {
    name: dockTitle,
    // Why: one stable Keychain key ('Orca Dev Safe Storage') for all dev
    // branches; the per-branch identity still shows via `name` (window title,
    // app menu, renderer label).
    appName: `${BASE_APP_NAME} Dev`,
    isDev: true,
    devLabel,
    devBranch: branch,
    devWorktreeName: worktreeName,
    devRepoRoot: repoRoot,
    dockBadgeLabel: null,
    appUserModelId: createDevAppUserModelId(repoRoot ?? devLabel)
  }
}
