#!/usr/bin/env node
// Symlinks the orca-dev wrapper into the developer's user-local bin directory
// so `pnpm run build:cli` never needs sudo or mutates a package-manager prefix.
import { existsSync, lstatSync, mkdirSync, readlinkSync, symlinkSync } from 'node:fs'
import path from 'node:path'

const scriptDir = import.meta.dirname
const source = path.join(scriptDir, 'orca-dev.mjs')
const homeDir = process.env.HOME ?? process.env.USERPROFILE
const supportsUserLocalBin = process.platform === 'darwin' || process.platform === 'linux'

const commandPath =
  supportsUserLocalBin && homeDir ? path.join(homeDir, '.local', 'bin', 'orca-dev') : null

if (!commandPath) {
  console.log(
    `[orca-dev] Skipping user-local symlink (${supportsUserLocalBin ? 'home directory unavailable' : 'unsupported platform'}).`
  )
  process.exit(0)
}

function isOwnedByUs(target) {
  try {
    if (!lstatSync(target).isSymbolicLink()) {
      return false
    }
    return readlinkSync(target) === source
  } catch {
    return false
  }
}

if (existsSync(commandPath)) {
  if (isOwnedByUs(commandPath)) {
    console.log(`[orca-dev] ${commandPath} already points to dev CLI.`)
    process.exit(0)
  }
  console.error(
    `[orca-dev] ${commandPath} exists but is not our symlink. Remove it manually if you want the dev CLI installed globally.`
  )
  process.exit(0)
}

try {
  mkdirSync(path.dirname(commandPath), { recursive: true })
  symlinkSync(source, commandPath)
  console.log(`[orca-dev] Symlinked ${commandPath} → ${source}`)
} catch (error) {
  // Why: the symlink is a local convenience; a read-only home must not make
  // the portable CLI or a release build fail after its artifacts are valid.
  console.error(
    `[orca-dev] Could not create ${commandPath}: ${error instanceof Error ? error.message : String(error)}`
  )
}
