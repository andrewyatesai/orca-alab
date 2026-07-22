import { readFileSync } from 'node:fs'
import path from 'node:path'
import { describe, expect, it } from 'vitest'
import { resolveNotificationStatusBundleId } from './notification-status-bundle-id.mjs'

describe('notification-status bundle id default', () => {
  // Why: macOS keys notification records to CFBundleIdentifier — a fork build
  // embedding upstream's id would read public Orca's notification authorization.
  it('defaults to the fork (.staging) identity, mirroring electron-builder appId', () => {
    expect(resolveNotificationStatusBundleId({})).toBe('com.stablyai.orca.staging')
  })

  it('restores the upstream identity only under ORCA_PUBLIC_IDENTITY=1', () => {
    expect(resolveNotificationStatusBundleId({ ORCA_PUBLIC_IDENTITY: '1' })).toBe(
      'com.stablyai.orca'
    )
    expect(resolveNotificationStatusBundleId({ ORCA_PUBLIC_IDENTITY: '0' })).toBe(
      'com.stablyai.orca.staging'
    )
    expect(resolveNotificationStatusBundleId({ ORCA_PUBLIC_IDENTITY: 'true' })).toBe(
      'com.stablyai.orca.staging'
    )
  })

  // Why: the build script cannot be imported without triggering a Swift build,
  // so pin its wiring by source — the default must come from the shared resolver,
  // never a hardcoded upstream id.
  it('build script derives its default from resolveNotificationStatusBundleId', () => {
    const scriptSource = readFileSync(
      path.join(import.meta.dirname, 'build-notification-status-macos.mjs'),
      'utf8'
    )
    expect(scriptSource).toContain("readArg('--bundle-id') ?? resolveNotificationStatusBundleId()")
    expect(scriptSource).not.toMatch(/\?\?\s*'com\.stablyai\.orca'/)
  })
})
