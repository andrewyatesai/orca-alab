import { resolve } from 'node:path'
import type { App } from 'electron'
import { describe, expect, it, vi } from 'vitest'
import {
  DEEP_LINK_DEV_REGISTRATION_ENV,
  registerOrcaProtocolClient
} from './deep-link-scheme-registration'

function makeFakeApp(): { app: App; setAsDefaultProtocolClient: ReturnType<typeof vi.fn> } {
  const setAsDefaultProtocolClient = vi.fn(() => true)
  return { app: { setAsDefaultProtocolClient } as unknown as App, setAsDefaultProtocolClient }
}

describe('registerOrcaProtocolClient', () => {
  it('registers the plain orca scheme for packaged builds', () => {
    const fake = makeFakeApp()

    const registered = registerOrcaProtocolClient(fake.app, {
      isServeMode: false,
      isDefaultApp: false
    })

    expect(registered).toBe(true)
    expect(fake.setAsDefaultProtocolClient).toHaveBeenCalledWith('orca')
  })

  it('registers nothing in serve mode', () => {
    const fake = makeFakeApp()

    const registered = registerOrcaProtocolClient(fake.app, {
      isServeMode: true,
      isDefaultApp: false
    })

    expect(registered).toBe(false)
    expect(fake.setAsDefaultProtocolClient).not.toHaveBeenCalled()
  })

  it('dev registration requires the env opt-in so a dev run cannot steal the installed handler', () => {
    const fake = makeFakeApp()

    const registered = registerOrcaProtocolClient(fake.app, {
      isServeMode: false,
      isDefaultApp: true,
      env: {},
      execPath: '/dev/electron',
      appArgvPath: './out/main'
    })

    expect(registered).toBe(false)
    expect(fake.setAsDefaultProtocolClient).not.toHaveBeenCalled()
  })

  it('opted-in dev registration passes the dev shim exec args', () => {
    const fake = makeFakeApp()

    const registered = registerOrcaProtocolClient(fake.app, {
      isServeMode: false,
      isDefaultApp: true,
      env: { [DEEP_LINK_DEV_REGISTRATION_ENV]: '1' },
      execPath: '/dev/electron',
      appArgvPath: './out/main'
    })

    expect(registered).toBe(true)
    expect(fake.setAsDefaultProtocolClient).toHaveBeenCalledWith('orca', '/dev/electron', [
      resolve('./out/main')
    ])
  })

  it('logs instead of throwing when registration fails (Linux without desktop integration)', () => {
    const setAsDefaultProtocolClient = vi.fn(() => {
      throw new Error('no desktop file')
    })
    const warn = vi.fn()

    const registered = registerOrcaProtocolClient(
      { setAsDefaultProtocolClient } as unknown as App,
      { isServeMode: false, isDefaultApp: false, warn }
    )

    expect(registered).toBe(false)
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('registration failed'))
  })
})
