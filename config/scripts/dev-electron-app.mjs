import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import {
  cpSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  readlinkSync,
  rmSync,
  symlinkSync,
  writeFileSync
} from 'node:fs'
import path from 'node:path'

function readGitValue(repoRoot, args) {
  try {
    const value = execFileSync('git', ['-C', repoRoot, ...args], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore']
    }).trim()
    return value || null
  } catch {
    return null
  }
}

function lastBranchSegment(value) {
  return value.replace(/\\/g, '/').split('/').findLast(Boolean) ?? value
}

export function formatDevInstanceLabel(branch, worktreeName) {
  if (branch && worktreeName) {
    if (branch === worktreeName || lastBranchSegment(branch) === worktreeName) {
      return worktreeName
    }
    return `${worktreeName} @ ${branch}`
  }
  return branch || worktreeName || null
}

const DEFAULT_DEV_DOCK_TITLE = 'Orca: ALab Edition'

export function seedDevInstanceIdentityEnv(repoRoot, env = process.env) {
  const branch =
    env.ORCA_DEV_BRANCH ||
    readGitValue(repoRoot, ['symbolic-ref', '--quiet', '--short', 'HEAD']) ||
    readGitValue(repoRoot, ['rev-parse', '--short', 'HEAD'])
  const worktreeName = env.ORCA_DEV_WORKTREE_NAME || path.basename(repoRoot)
  const label = env.ORCA_DEV_INSTANCE_LABEL || formatDevInstanceLabel(branch, worktreeName)
  const identitySeed = env.ORCA_DEV_INSTANCE_KEY || repoRoot
  const dockTitle = env.ORCA_DEV_DOCK_TITLE || DEFAULT_DEV_DOCK_TITLE

  env.ORCA_DEV_REPO_ROOT ||= repoRoot
  env.ORCA_DEV_INSTANCE_KEY ||= identitySeed
  if (branch) {
    env.ORCA_DEV_BRANCH ||= branch
  }
  if (worktreeName) {
    env.ORCA_DEV_WORKTREE_NAME ||= worktreeName
  }
  if (label) {
    // Why: parallel dev runs need a stable origin label for window titles,
    // Dock names, and automation sessions without re-running git in Electron.
    env.ORCA_DEV_INSTANCE_LABEL ||= label
  }
  env.ORCA_DEV_DOCK_TITLE ||= dockTitle
}

function setPlistValue(plistPath, key, value, execFile) {
  execFile('/usr/bin/plutil', ['-replace', key, '-string', value, plistPath])
}

export function sanitizeMacAppBundleName(value) {
  return (
    Array.from(value, (char) => {
      const code = char.charCodeAt(0)
      return code < 32 || code === 127 || char === '/' || char === '\\' ? '-' : char
    })
      .join('')
      .replace(/\s+/g, ' ')
      .trim()
      .slice(0, 120) || 'Orca'
  )
}

export function isManagedDevElectronExecutable(repoRoot, executablePath) {
  if (!executablePath) {
    return true
  }

  const resolvedPath = path.resolve(executablePath)
  const stockPaths = [
    path.join(repoRoot, 'node_modules', '.bin', 'electron'),
    path.join(
      repoRoot,
      'node_modules',
      'electron',
      'dist',
      'Electron.app',
      'Contents',
      'MacOS',
      'Electron'
    )
  ]
  if (stockPaths.some((stockPath) => resolvedPath === path.resolve(stockPath))) {
    return true
  }

  const managedRoot = `${path.resolve(repoRoot, 'out', 'electron-dev')}${path.sep}`
  return resolvedPath.startsWith(managedRoot)
}

export function prepareMacDevElectronApp(
  repoRoot,
  {
    env = process.env,
    platform = process.platform,
    execFile = execFileSync,
    sourceAppPath = path.join(repoRoot, 'node_modules', 'electron', 'dist', 'Electron.app'),
    electronPackagePath = path.join(repoRoot, 'node_modules', 'electron', 'package.json')
  } = {}
) {
  if (platform !== 'darwin' || !existsSync(sourceAppPath)) {
    return null
  }

  let electronVersion = null
  try {
    electronVersion = JSON.parse(readFileSync(electronPackagePath, 'utf8')).version ?? null
  } catch {}

  const title = env.ORCA_DEV_DOCK_TITLE || DEFAULT_DEV_DOCK_TITLE
  const identityKey = env.ORCA_DEV_INSTANCE_KEY || repoRoot
  // v6: bundle the notification-status helper (real permission readout) and
  // ad-hoc re-sign after plist edits so Notification Center accepts the
  // bundle; bumping forces stale cached copies to be recreated.
  const bundleLayoutVersion = 'dock-title-app-preserve-framework-symlinks-v6'
  const hash = createHash('sha1')
    .update(
      `${sourceAppPath}\0${electronVersion ?? ''}\0${title}\0${identityKey}\0${bundleLayoutVersion}`
    )
    .digest('hex')
    .slice(0, 12)
  const distDir = path.join(repoRoot, 'out', 'electron-dev', hash)
  // Why: macOS uses the bundle's filesystem display name for direct launches
  // from electron-vite and the CLI, even when Info.plist is patched.
  const appBundleName = `${sanitizeMacAppBundleName(title)}.app`
  const appPath = path.join(distDir, appBundleName)
  const markerPath = path.join(distDir, 'orca-dev-electron-app.json')
  // Why: one stable id for every dev instance. Per-instance ids registered a
  // new macOS Notification Settings entry for each branch x Electron version.
  const bundleId = 'com.stablyai.orca.dev'
  env.ORCA_DEV_MACOS_BUNDLE_ID = bundleId
  const expectedMarker = JSON.stringify(
    { title, appBundleName, bundleId, sourceAppPath, electronVersion, bundleLayoutVersion },
    null,
    2
  )
  const executablePath = path.join(appPath, 'Contents', 'MacOS', 'Electron')
  const requiredResourcePaths = [
    path.join(
      appPath,
      'Contents',
      'Frameworks',
      'Electron Framework.framework',
      'Resources',
      'icudtl.dat'
    )
  ]

  function copiedAppIsUsable() {
    if (!existsSync(markerPath) || !existsSync(appPath)) {
      return false
    }
    try {
      if (readFileSync(markerPath, 'utf8') !== expectedMarker) {
        return false
      }
    } catch {
      return false
    }
    // Why: a previous interrupted copy can leave the marker and executable
    // present but miss Chromium framework resources, causing a blank crash.
    return (
      existsSync(executablePath) &&
      requiredResourcePaths.every((resourcePath) => existsSync(resourcePath))
    )
  }

  if (copiedAppIsUsable()) {
    env.ELECTRON_EXEC_PATH = executablePath
    return executablePath
  }

  rmSync(distDir, { recursive: true, force: true })
  mkdirSync(distDir, { recursive: true })
  // Why: Electron.framework uses relative symlinks for its bundle resources;
  // resolving them to pnpm-store absolutes breaks Chromium's bundle lookup.
  cpSync(sourceAppPath, appPath, { recursive: true, verbatimSymlinks: true })
  restoreElectronFrameworkSymlinks(appPath)

  const plistPath = path.join(appPath, 'Contents', 'Info.plist')
  setPlistValue(plistPath, 'CFBundleName', title, execFile)
  setPlistValue(plistPath, 'CFBundleDisplayName', title, execFile)
  setPlistValue(plistPath, 'CFBundleIdentifier', bundleId, execFile)

  // Why: the helper reads the app's real macOS notification authorization.
  // Non-fatal: without swiftc the permission card falls back to probes.
  try {
    execFile(
      process.execPath,
      [
        path.join(repoRoot, 'config', 'scripts', 'build-notification-status-macos.mjs'),
        '--bundle-id',
        bundleId,
        '--single-arch',
        '--output',
        path.join(appPath, 'Contents', 'MacOS', 'orca-notification-status')
      ],
      { stdio: 'inherit' }
    )
  } catch (error) {
    console.warn(
      `[orca-dev] notification-status helper build failed (permission card falls back to probes): ${error?.message ?? error}`
    )
  }

  // Why: plist edits break the bundle seal. An ad-hoc re-sign restores dev
  // notification delivery and lets LaunchServices trust the patched identity.
  try {
    execFile('/usr/bin/codesign', ['--force', '--deep', '--sign', '-', appPath])
  } catch (error) {
    console.warn(
      `[orca-dev] ad-hoc codesign failed (dev notifications will not deliver): ${error?.message ?? error}`
    )
  }
  writeFileSync(markerPath, expectedMarker, 'utf8')
  env.ELECTRON_EXEC_PATH = executablePath
  return executablePath
}

function isSymlink(filePath) {
  try {
    return lstatSync(filePath).isSymbolicLink()
  } catch {
    return false
  }
}

function ensureRelativeSymlink(linkPath, target) {
  if (isSymlink(linkPath)) {
    try {
      if (readlinkSync(linkPath) === target) {
        return
      }
    } catch {}
  }

  const targetPath = path.join(path.dirname(linkPath), target)
  if (!existsSync(targetPath)) {
    return
  }

  rmSync(linkPath, { recursive: true, force: true })
  symlinkSync(target, linkPath)
}

function restoreElectronFrameworkSymlinks(appPath) {
  const frameworkPath = path.join(appPath, 'Contents', 'Frameworks', 'Electron Framework.framework')
  const versionsPath = path.join(frameworkPath, 'Versions')
  if (!existsSync(path.join(versionsPath, 'A'))) {
    return
  }

  // Why: some Electron installs have framework symlinks flattened into
  // duplicate directories. Restore the canonical relative bundle links.
  ensureRelativeSymlink(path.join(versionsPath, 'Current'), 'A')
  for (const entry of ['Electron Framework', 'Resources', 'Libraries', 'Helpers']) {
    ensureRelativeSymlink(path.join(frameworkPath, entry), `Versions/Current/${entry}`)
  }
}
