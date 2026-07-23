import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  createStoredWebRuntimeEnvironment,
  readStoredWebRuntimeEnvironment,
  redactStoredWebRuntimeEnvironment,
  type StoredWebRuntimeEnvironment
} from './web-runtime-environment'
import type { WebPairingOffer } from './web-pairing'

const STORAGE_KEY = 'orca.web.runtimeEnvironment.v1'

class MemoryStorage {
  private readonly values = new Map<string, string>()
  getItem(key: string): string | null {
    return this.values.get(key) ?? null
  }
  setItem(key: string, value: string): void {
    this.values.set(key, value)
  }
  removeItem(key: string): void {
    this.values.delete(key)
  }
}

function offer(overrides: Partial<WebPairingOffer> = {}): WebPairingOffer {
  return {
    v: 2,
    endpoint: 'ws://127.0.0.1:6768',
    deviceToken: 'token',
    publicKeyB64: 'server-key',
    ...overrides
  }
}

function previousEnvironment(
  overrides: Partial<StoredWebRuntimeEnvironment> = {}
): StoredWebRuntimeEnvironment {
  return {
    id: 'web-prior',
    name: 'Orca Server',
    createdAt: 1,
    updatedAt: 1,
    lastUsedAt: null,
    runtimeId: null,
    preferredEndpointId: 'ws-web-prior',
    endpoints: [
      {
        id: 'ws-web-prior',
        kind: 'websocket',
        label: 'WebSocket',
        endpoint: 'ws://127.0.0.1:6768',
        deviceToken: 'token',
        publicKeyB64: 'server-key'
      }
    ],
    ...overrides
  }
}

describe('web runtime environment ownership provenance (#9776)', () => {
  beforeEach(() => {
    vi.stubGlobal('window', { localStorage: new MemoryStorage() })
  })
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('carries the prior env id when re-pairing to the same server publicKey', () => {
    const previous = previousEnvironment()
    const next = createStoredWebRuntimeEnvironment({
      name: 'Orca Server',
      offer: offer(),
      previousEnvironment: previous
    })
    expect(next.id).not.toBe(previous.id)
    expect(next.compatibleEnvironmentIds).toEqual(['web-prior'])
  })

  it('accumulates the ancestor chain across repeated re-pairing', () => {
    const previous = previousEnvironment({
      id: 'web-second',
      compatibleEnvironmentIds: ['web-first']
    })
    const next = createStoredWebRuntimeEnvironment({
      name: 'Orca Server',
      offer: offer(),
      previousEnvironment: previous
    })
    expect(next.compatibleEnvironmentIds).toEqual(['web-first', 'web-second'])
  })

  it('does not carry provenance when the server publicKey differs', () => {
    const next = createStoredWebRuntimeEnvironment({
      name: 'Orca Server',
      offer: offer({ publicKeyB64: 'a-different-server' }),
      previousEnvironment: previousEnvironment()
    })
    expect(next.compatibleEnvironmentIds).toBeUndefined()
  })

  it('omits provenance entirely when there is no previous environment', () => {
    const next = createStoredWebRuntimeEnvironment({ name: 'Orca Server', offer: offer() })
    expect('compatibleEnvironmentIds' in next).toBe(false)
  })

  it('never leaks compatibleEnvironmentIds through the redacted public shape', () => {
    const redacted = redactStoredWebRuntimeEnvironment(
      previousEnvironment({ compatibleEnvironmentIds: ['web-first'] })
    )
    expect('compatibleEnvironmentIds' in redacted).toBe(false)
  })

  it('filters non-string compatibleEnvironmentIds when reading persisted state', () => {
    const stored = {
      ...previousEnvironment(),
      compatibleEnvironmentIds: ['web-first', 42, null, 'web-second']
    }
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(stored))
    const parsed = readStoredWebRuntimeEnvironment()
    expect(parsed?.compatibleEnvironmentIds).toEqual(['web-first', 'web-second'])
  })

  it('returns null for a persisted environment whose endpoints are not an array', () => {
    window.localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({ id: 'web-x', name: 'Orca Server', endpoints: 'nope' })
    )
    expect(readStoredWebRuntimeEnvironment()).toBeNull()
  })
})
