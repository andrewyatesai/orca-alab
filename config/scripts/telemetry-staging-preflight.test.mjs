import { spawnSync } from 'node:child_process'
import path from 'node:path'
import { describe, expect, it } from 'vitest'
import { runStagingTelemetryPreflight } from './telemetry-staging-preflight.mjs'

const FORK_KEY = 'phc_fork1234567890abcdef'

describe('runStagingTelemetryPreflight', () => {
  it('passes non-staging builds without requiring any telemetry env', () => {
    const result = runStagingTelemetryPreflight({ version: '1.4.122-rc.3', env: {} })
    expect(result).toMatchObject({ ok: true, staging: false, dark: false })
  })

  it('detects staging from a -fork. version suffix', () => {
    const result = runStagingTelemetryPreflight({ version: '1.4.122-fork.1', env: {} })
    expect(result.staging).toBe(true)
    expect(result.ok).toBe(false)
  })

  it('detects staging from ORCA_STAGING=1', () => {
    const result = runStagingTelemetryPreflight({
      version: '1.4.122-rc.3',
      env: { ORCA_STAGING: '1' }
    })
    expect(result.staging).toBe(true)
    expect(result.ok).toBe(false)
  })

  it('fails loudly when a staging build has no write key', () => {
    const result = runStagingTelemetryPreflight({
      version: '1.4.122-fork.1',
      env: {}
    })
    expect(result.ok).toBe(false)
    expect(result.errors.join('\n')).toContain('ORCA_POSTHOG_WRITE_KEY')
  })

  it('allows a keyless staging build only via the explicit dark-staging opt-in', () => {
    const result = runStagingTelemetryPreflight({
      version: '1.4.122-fork.1',
      env: { ORCA_ALLOW_NO_TELEMETRY: '1' }
    })
    expect(result).toMatchObject({ ok: true, staging: true, dark: true })
    expect(result.messages.join('\n')).toContain('DARK STAGING')
  })

  it('passes a fully keyed staging build', () => {
    const result = runStagingTelemetryPreflight({
      version: '1.4.122-fork.1',
      env: { ORCA_POSTHOG_WRITE_KEY: FORK_KEY, ORCA_BUILD_IDENTITY: 'rc' }
    })
    expect(result).toMatchObject({ ok: true, staging: true, dark: false })
  })

  it('rejects a malformed write key', () => {
    const result = runStagingTelemetryPreflight({
      version: '1.4.122-fork.1',
      env: { ORCA_POSTHOG_WRITE_KEY: 'not-a-posthog-key', ORCA_BUILD_IDENTITY: 'rc' }
    })
    expect(result.ok).toBe(false)
    expect(result.errors.join('\n')).toContain('not a valid PostHog project key')
  })

  it('rejects a key without a build identity (one-without-other fails closed at runtime)', () => {
    const result = runStagingTelemetryPreflight({
      version: '1.4.122-fork.1',
      env: { ORCA_POSTHOG_WRITE_KEY: FORK_KEY }
    })
    expect(result.ok).toBe(false)
    expect(result.errors.join('\n')).toContain('ORCA_BUILD_IDENTITY')
  })

  it('rejects a key that does not match the pinned fork prefix (public-key guardrail)', () => {
    const result = runStagingTelemetryPreflight({
      version: '1.4.122-fork.1',
      env: {
        ORCA_POSTHOG_WRITE_KEY: 'phc_public9999999999',
        ORCA_BUILD_IDENTITY: 'rc',
        ORCA_FORK_POSTHOG_KEY_PREFIX: 'phc_fork'
      }
    })
    expect(result.ok).toBe(false)
    expect(result.errors.join('\n')).toContain('foreign')
  })

  it('accepts a key matching the pinned fork prefix', () => {
    const result = runStagingTelemetryPreflight({
      version: '1.4.122-fork.1',
      env: {
        ORCA_POSTHOG_WRITE_KEY: FORK_KEY,
        ORCA_BUILD_IDENTITY: 'rc',
        ORCA_FORK_POSTHOG_KEY_PREFIX: 'phc_fork'
      }
    })
    expect(result.ok).toBe(true)
  })
})

// End-to-end CLI smoke of the `--preflight` wiring in
// verify-telemetry-constants.mjs — the exact command the build:desktop /
// build:release legs run. Uses process.execPath so it works on Windows
// runners without a shell.
describe('verify-telemetry-constants --preflight CLI', () => {
  const script = path.resolve(import.meta.dirname, 'verify-telemetry-constants.mjs')

  function runPreflight(extraEnv) {
    // Strip ambient ORCA_* vars so a developer's shell cannot flip outcomes,
    // and pin the version via the test hook so the suite stays deterministic
    // whatever the repo's live package.json version is (rc vs -fork.N).
    const env = { ...process.env }
    delete env.ORCA_STAGING
    delete env.ORCA_POSTHOG_WRITE_KEY
    delete env.ORCA_BUILD_IDENTITY
    delete env.ORCA_ALLOW_NO_TELEMETRY
    delete env.ORCA_FORK_POSTHOG_KEY_PREFIX
    env.ORCA_TELEMETRY_PREFLIGHT_VERSION = '1.4.122-rc.3'
    Object.assign(env, extraEnv)
    return spawnSync(process.execPath, [script, '--preflight'], { env, encoding: 'utf8' })
  }

  it('exits 0 for a non-staging build', () => {
    const result = runPreflight({})
    expect(result.status).toBe(0)
  })

  it('exits 1 for a keyless staging build', () => {
    const result = runPreflight({ ORCA_STAGING: '1' })
    expect(result.status).toBe(1)
    expect(result.stderr).toContain('ORCA_POSTHOG_WRITE_KEY')
  })

  it('exits 0 for a dark-staging opt-in with a loud warning', () => {
    const result = runPreflight({ ORCA_STAGING: '1', ORCA_ALLOW_NO_TELEMETRY: '1' })
    expect(result.status).toBe(0)
    expect(result.stdout).toContain('DARK STAGING')
  })

  it('exits 0 for a keyed staging build', () => {
    const result = runPreflight({
      ORCA_STAGING: '1',
      ORCA_POSTHOG_WRITE_KEY: FORK_KEY,
      ORCA_BUILD_IDENTITY: 'rc'
    })
    expect(result.status).toBe(0)
  })

  it('honors the -fork. version detection through the version test hook', () => {
    const result = runPreflight({ ORCA_TELEMETRY_PREFLIGHT_VERSION: '1.4.122-fork.1' })
    expect(result.status).toBe(1)
  })
})
