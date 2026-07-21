import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  formatDevInstanceLabel,
  isManagedDevElectronExecutable,
  prepareMacDevElectronApp,
  sanitizeMacAppBundleName,
  seedDevInstanceIdentityEnv
} from './dev-electron-app.mjs'

const temporaryRoots = []

afterEach(() => {
  for (const root of temporaryRoots.splice(0)) {
    rmSync(root, { recursive: true, force: true })
  }
})

function makeTemporaryRoot() {
  const root = mkdtempSync(path.join(tmpdir(), 'orca-dev-electron-app-'))
  temporaryRoots.push(root)
  return root
}

function createElectronAppFixture(root) {
  const sourceAppPath = path.join(root, 'Electron.app')
  const executablePath = path.join(sourceAppPath, 'Contents', 'MacOS', 'Electron')
  const frameworkResourcesPath = path.join(
    sourceAppPath,
    'Contents',
    'Frameworks',
    'Electron Framework.framework',
    'Resources'
  )
  mkdirSync(path.dirname(executablePath), { recursive: true })
  mkdirSync(frameworkResourcesPath, { recursive: true })
  writeFileSync(executablePath, 'electron fixture', 'utf8')
  writeFileSync(path.join(sourceAppPath, 'Contents', 'Info.plist'), 'plist fixture', 'utf8')
  writeFileSync(path.join(frameworkResourcesPath, 'icudtl.dat'), 'icu fixture', 'utf8')

  const electronPackagePath = path.join(root, 'electron-package.json')
  writeFileSync(electronPackagePath, JSON.stringify({ version: '99.1.0' }), 'utf8')
  return { sourceAppPath, electronPackagePath }
}

describe('dev Electron app identity', () => {
  it('formats stable Orca dev names and filesystem-safe bundle names', () => {
    expect(formatDevInstanceLabel('feature/menu', 'orc')).toBe('orc @ feature/menu')
    expect(formatDevInstanceLabel('feature/orc', 'orc')).toBe('orc')
    expect(sanitizeMacAppBundleName('Orca: feature/menu\\test\n')).toBe('Orca: feature-menu-test-')
  })

  it('seeds the identity inherited by CLI-launched Electron', () => {
    const env = {
      ORCA_DEV_BRANCH: 'feature/menu',
      ORCA_DEV_WORKTREE_NAME: 'orc'
    }

    seedDevInstanceIdentityEnv('/workspace/orc', env)

    expect(env).toMatchObject({
      ORCA_DEV_REPO_ROOT: '/workspace/orc',
      ORCA_DEV_INSTANCE_KEY: '/workspace/orc',
      ORCA_DEV_INSTANCE_LABEL: 'orc @ feature/menu',
      ORCA_DEV_DOCK_TITLE: 'Orca: ALab Edition'
    })
  })

  it('recognizes stock and copied dev executables without replacing custom apps', () => {
    const repoRoot = '/workspace/orc'
    expect(isManagedDevElectronExecutable(repoRoot)).toBe(true)
    expect(
      isManagedDevElectronExecutable(repoRoot, '/workspace/orc/node_modules/.bin/electron')
    ).toBe(true)
    expect(
      isManagedDevElectronExecutable(
        repoRoot,
        '/workspace/orc/out/electron-dev/123/Orca.app/Contents/MacOS/Electron'
      )
    ).toBe(true)
    expect(
      isManagedDevElectronExecutable(repoRoot, '/Applications/Custom.app/Contents/MacOS/App')
    ).toBe(false)
  })

  it('copies and patches a reusable Orca-named app bundle for CLI launches', () => {
    const repoRoot = makeTemporaryRoot()
    const { sourceAppPath, electronPackagePath } = createElectronAppFixture(repoRoot)
    const env = {
      ORCA_DEV_DOCK_TITLE: 'Orca: feature/menu',
      ORCA_DEV_INSTANCE_KEY: repoRoot
    }
    const execFile = vi.fn()

    const executablePath = prepareMacDevElectronApp(repoRoot, {
      env,
      platform: 'darwin',
      execFile,
      sourceAppPath,
      electronPackagePath
    })

    expect(executablePath).toContain(
      `${path.sep}Orca: feature-menu.app${path.sep}Contents${path.sep}MacOS${path.sep}Electron`
    )
    expect(readFileSync(executablePath, 'utf8')).toBe('electron fixture')
    expect(env).toMatchObject({
      ELECTRON_EXEC_PATH: executablePath,
      ORCA_DEV_MACOS_BUNDLE_ID: 'com.stablyai.orca.dev'
    })
    expect(execFile).toHaveBeenCalledWith(
      '/usr/bin/plutil',
      expect.arrayContaining(['CFBundleName', 'Orca: feature/menu'])
    )
    expect(execFile).toHaveBeenCalledWith(
      '/usr/bin/plutil',
      expect.arrayContaining(['CFBundleDisplayName', 'Orca: feature/menu'])
    )
    expect(execFile).toHaveBeenCalledWith(
      '/usr/bin/codesign',
      expect.arrayContaining(['--sign', '-'])
    )

    execFile.mockClear()
    expect(
      prepareMacDevElectronApp(repoRoot, {
        env,
        platform: 'darwin',
        execFile,
        sourceAppPath,
        electronPackagePath
      })
    ).toBe(executablePath)
    expect(execFile).not.toHaveBeenCalled()
  })

  it('does nothing outside macOS', () => {
    expect(
      prepareMacDevElectronApp('/workspace/orc', {
        env: {},
        platform: 'linux'
      })
    ).toBeNull()
  })
})
