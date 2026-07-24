import { runInNewContext } from 'node:vm'
import { describe, expect, it } from 'vitest'

import { ANTI_DETECTION_SCRIPT } from './anti-detection'

type PermissionQueryResult = {
  state: string
  onchange: null
}

type AntiDetectionContext = {
  Notification: {
    permission: string
    requestPermission: (callback?: (permission: string) => void) => Promise<string>
  }
  navigator: {
    permissions: {
      query: (descriptor: { name: string }) => Promise<PermissionQueryResult>
    }
  }
}

function createContext(args: {
  nativeNotificationPermission: string
  requestedNotificationPermission: string
}): AntiDetectionContext & Record<string, unknown> {
  class Permissions {
    query(): Promise<PermissionQueryResult> {
      return Promise.resolve({ state: 'denied', onchange: null })
    }
  }

  const Notification = {
    permission: args.nativeNotificationPermission,
    requestPermission(callback?: (permission: string) => void): Promise<string> {
      callback?.(args.requestedNotificationPermission)
      return Promise.resolve(args.requestedNotificationPermission)
    }
  }
  Object.defineProperty(Notification, 'permission', {
    configurable: true,
    get: () => args.nativeNotificationPermission
  })

  return {
    Date,
    Object,
    Promise,
    Set,
    performance: { now: () => 0 },
    window: {},
    navigator: {
      plugins: [],
      languages: [],
      permissions: new Permissions()
    },
    Permissions,
    Notification
  } as AntiDetectionContext & Record<string, unknown>
}

describe('ANTI_DETECTION_SCRIPT', () => {
  it('reports notification permission as granted after a site permission request succeeds', async () => {
    const context = createContext({
      nativeNotificationPermission: 'denied',
      requestedNotificationPermission: 'granted'
    })

    runInNewContext(ANTI_DETECTION_SCRIPT, context)

    expect(context.Notification.permission).toBe('default')
    await expect(context.navigator.permissions.query({ name: 'notifications' })).resolves.toEqual({
      state: 'prompt',
      onchange: null
    })

    await expect(context.Notification.requestPermission()).resolves.toBe('granted')

    expect(context.Notification.permission).toBe('granted')
    await expect(context.navigator.permissions.query({ name: 'notifications' })).resolves.toEqual({
      state: 'granted',
      onchange: null
    })
  })

  it('returns a real PermissionStatus (EventTarget) for prompt permissions, only overriding state', async () => {
    // Regression: fabricating { state, onchange } dropped addEventListener, so a
    // page doing query('camera').addEventListener('change', ...) threw TypeError.
    const changeHandler = (): void => {}
    class Permissions {
      query(desc: { name: string }): Promise<Record<string, unknown>> {
        // A faithful native result: an EventTarget-like object with the API.
        return Promise.resolve({
          name: desc.name,
          state: 'denied',
          onchange: null,
          addEventListener: changeHandler,
          removeEventListener: changeHandler,
          dispatchEvent: () => true
        })
      }
    }
    const Notification = {} as Record<string, unknown>
    Object.defineProperty(Notification, 'permission', { configurable: true, get: () => 'default' })
    const context = {
      Date,
      Object,
      Promise,
      Set,
      performance: { now: () => 0 },
      window: {},
      navigator: { plugins: [], languages: [], permissions: new Permissions() },
      Permissions,
      Notification
    } as Record<string, unknown>

    runInNewContext(ANTI_DETECTION_SCRIPT, context)

    const status = (await (
      context.navigator as {
        permissions: { query: (d: { name: string }) => Promise<Record<string, unknown>> }
      }
    ).permissions.query({ name: 'camera' })) as Record<string, unknown>

    expect(status.state).toBe('prompt')
    expect(typeof status.addEventListener).toBe('function')
    expect(() =>
      (status.addEventListener as (t: string, cb: () => void) => void)('change', () => {})
    ).not.toThrow()
  })

  it('preserves notification permission when Electron already reports a grant', async () => {
    const context = createContext({
      nativeNotificationPermission: 'granted',
      requestedNotificationPermission: 'granted'
    })

    runInNewContext(ANTI_DETECTION_SCRIPT, context)

    expect(context.Notification.permission).toBe('granted')
    await expect(context.navigator.permissions.query({ name: 'notifications' })).resolves.toEqual({
      state: 'granted',
      onchange: null
    })
  })
})
