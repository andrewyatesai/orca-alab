import { afterAll, afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { rmSync, statSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import type * as NodeOs from 'node:os'

const hoisted = vi.hoisted(() => ({ home: '' }))

// Why: OAUTH_CREDS_PATH is derived from homedir() at module load, so redirect
// homedir to a real temp dir created inside the mock factory (before the module
// under test is imported) so the atomic write runs and its on-disk permission
// bits can be asserted.
vi.mock('node:os', async (importOriginal) => {
  const actual = await importOriginal<typeof NodeOs>()
  const fs = await import('node:fs')
  const nodePath = await import('node:path')
  const home = fs.mkdtempSync(nodePath.join(actual.tmpdir(), 'gemini-creds-'))
  fs.mkdirSync(nodePath.join(home, '.gemini'), { recursive: true })
  hoisted.home = home
  return { ...actual, homedir: () => home }
})

import { saveGeminiCredentials } from './gemini-oauth-sources'

const creds = {
  access_token: 'access-abc',
  refresh_token: 'refresh-xyz',
  expiry_date: Date.now() + 3600_000
}

describe('saveGeminiCredentials permissions', () => {
  const credsPath = () => path.join(hoisted.home, '.gemini', 'oauth_creds.json')

  beforeEach(() => {
    rmSync(credsPath(), { force: true })
  })

  afterEach(() => {
    rmSync(credsPath(), { force: true })
  })

  afterAll(() => {
    rmSync(hoisted.home, { recursive: true, force: true })
  })

  // Why: mode bits are POSIX-only; Windows does not model 0o600.
  it.skipIf(process.platform === 'win32')('writes OAuth tokens owner-only (0o600)', async () => {
    await saveGeminiCredentials(creds)
    expect(statSync(credsPath()).mode & 0o777).toBe(0o600)
  })

  it.skipIf(process.platform === 'win32')(
    'tightens a pre-existing world-readable credential file',
    async () => {
      writeFileSync(credsPath(), '{}', { mode: 0o644 })
      expect(statSync(credsPath()).mode & 0o777).toBe(0o644)
      await saveGeminiCredentials(creds)
      expect(statSync(credsPath()).mode & 0o777).toBe(0o600)
    }
  )
})
