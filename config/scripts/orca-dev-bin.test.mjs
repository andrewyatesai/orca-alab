import { execFileSync } from 'node:child_process'
import { chmodSync, cpSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { describe, expect, it } from 'vitest'

const projectDir = path.resolve(import.meta.dirname, '../..')
const packageJson = JSON.parse(readFileSync(path.join(projectDir, 'package.json'), 'utf8'))
const wrapperPath = path.join(projectDir, 'config', 'scripts', 'orca-dev.mjs')

describe('orca-dev package bin', () => {
  it('uses a Node entrypoint for cross-platform package installs', () => {
    expect(packageJson.bin['orca-dev']).toBe('./config/scripts/orca-dev.mjs')
    expect(readFileSync(wrapperPath, 'utf8')).toMatch(/^#!\/usr\/bin\/env node\n/)
  })

  it('runs the dev CLI through Node without requiring Bash', () => {
    const root = mkdtempSync(path.join(tmpdir(), 'orca-dev-bin-'))
    const cliEntry = path.join(root, 'cli-entry.cjs')
    const outputPath = path.join(root, 'output.json')
    writeFileSync(
      cliEntry,
      [
        'const fs = require("node:fs");',
        `fs.writeFileSync(${JSON.stringify(outputPath)}, JSON.stringify({`,
        '  argv: process.argv.slice(2),',
        '  userDataPath: process.env.ORCA_USER_DATA_PATH,',
        '  appExecutable: process.env.ORCA_APP_EXECUTABLE',
        '}));'
      ].join('\n'),
      'utf8'
    )
    if (process.platform !== 'win32') {
      chmodSync(cliEntry, 0o755)
    }

    execFileSync(process.execPath, [wrapperPath, '--help'], {
      env: {
        ...process.env,
        ORCA_DEV_CLI_ENTRY_PATH: cliEntry,
        ORCA_DEV_USER_DATA_PATH: path.join(root, 'user-data'),
        ORCA_APP_EXECUTABLE: path.join(root, 'Electron')
      },
      stdio: 'ignore'
    })

    expect(JSON.parse(readFileSync(outputPath, 'utf8'))).toEqual({
      argv: ['--help'],
      userDataPath: path.join(root, 'user-data'),
      appExecutable: path.join(root, 'Electron')
    })
  })

  it.runIf(process.platform === 'darwin')(
    'points macOS launch commands at an Orca-named app bundle',
    () => {
      const root = mkdtempSync(path.join(tmpdir(), 'orca-dev-bin-mac-'))
      const scriptsDir = path.join(root, 'config', 'scripts')
      mkdirSync(scriptsDir, { recursive: true })
      const isolatedWrapperPath = path.join(scriptsDir, 'orca-dev.mjs')
      cpSync(wrapperPath, isolatedWrapperPath)
      cpSync(
        path.join(projectDir, 'config', 'scripts', 'dev-electron-app.mjs'),
        path.join(scriptsDir, 'dev-electron-app.mjs')
      )

      const sourceAppPath = path.join(root, 'node_modules', 'electron', 'dist', 'Electron.app')
      const sourceExecutablePath = path.join(sourceAppPath, 'Contents', 'MacOS', 'Electron')
      const frameworkResourcesPath = path.join(
        sourceAppPath,
        'Contents',
        'Frameworks',
        'Electron Framework.framework',
        'Resources'
      )
      mkdirSync(path.dirname(sourceExecutablePath), { recursive: true })
      mkdirSync(frameworkResourcesPath, { recursive: true })
      writeFileSync(sourceExecutablePath, '#!/bin/sh\nexit 0\n', 'utf8')
      chmodSync(sourceExecutablePath, 0o755)
      writeFileSync(path.join(frameworkResourcesPath, 'icudtl.dat'), 'fixture', 'utf8')
      writeFileSync(
        path.join(sourceAppPath, 'Contents', 'Info.plist'),
        [
          '<?xml version="1.0" encoding="UTF-8"?>',
          '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">',
          '<plist version="1.0"><dict>',
          '<key>CFBundleName</key><string>Electron</string>',
          '<key>CFBundleDisplayName</key><string>Electron</string>',
          '<key>CFBundleIdentifier</key><string>com.github.Electron</string>',
          '<key>CFBundleExecutable</key><string>Electron</string>',
          '<key>CFBundlePackageType</key><string>APPL</string>',
          '</dict></plist>'
        ].join(''),
        'utf8'
      )
      writeFileSync(
        path.join(root, 'node_modules', 'electron', 'package.json'),
        JSON.stringify({ version: '99.1.0' }),
        'utf8'
      )

      const cliEntry = path.join(root, 'cli-entry.cjs')
      const outputPath = path.join(root, 'output.json')
      writeFileSync(
        cliEntry,
        [
          'const fs = require("node:fs");',
          `fs.writeFileSync(${JSON.stringify(outputPath)}, JSON.stringify({`,
          '  appExecutable: process.env.ORCA_APP_EXECUTABLE,',
          '  appNeedsRoot: process.env.ORCA_APP_EXECUTABLE_NEEDS_APP_ROOT',
          '}));'
        ].join('\n'),
        'utf8'
      )

      const env = {
        ...process.env,
        ORCA_DEV_CLI_ENTRY_PATH: cliEntry,
        ORCA_DEV_USER_DATA_PATH: path.join(root, 'user-data'),
        ORCA_DEV_BRANCH: 'main',
        ORCA_DEV_WORKTREE_NAME: 'orc'
      }
      delete env.ORCA_APP_EXECUTABLE
      delete env.ORCA_DEV_STABLE_NAME
      delete env.ORCA_SKIP_DEV_ELECTRON_APP_PREPARE
      execFileSync(process.execPath, [isolatedWrapperPath, 'open'], {
        env,
        stdio: 'ignore'
      })

      const output = JSON.parse(readFileSync(outputPath, 'utf8'))
      expect(output).toMatchObject({ appNeedsRoot: '1' })
      expect(output.appExecutable).toContain(`${path.sep}out${path.sep}electron-dev${path.sep}`)
      expect(output.appExecutable).toContain(
        `${path.sep}Orca: ALab Edition.app${path.sep}Contents${path.sep}MacOS${path.sep}Electron`
      )
      const appPath = output.appExecutable.slice(
        0,
        output.appExecutable.indexOf(`${path.sep}Contents${path.sep}MacOS${path.sep}`)
      )
      expect(
        execFileSync(
          '/usr/bin/plutil',
          ['-extract', 'CFBundleName', 'raw', path.join(appPath, 'Contents', 'Info.plist')],
          { encoding: 'utf8' }
        ).trim()
      ).toBe('Orca: ALab Edition')
    }
  )
})
