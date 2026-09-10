import { afterEach, describe, expect, it, vi } from 'vitest'
import { connect } from './rpc-client'
import { redactedEndpoint } from './rpc-client-connection-log'
import type { ConnectionLogEntry } from './types'

vi.mock('./e2ee', () => ({
  generateKeyPair: () => ({
    publicKey: new Uint8Array(32),
    secretKey: new Uint8Array(32)
  }),
  deriveSharedKey: () => new Uint8Array(32),
  publicKeyFromBase64: () => new Uint8Array(32),
  publicKeyToBase64: () => 'client-public-key',
  encrypt: (plaintext: string) => plaintext,
  decrypt: (raw: string) => raw,
  decryptBytes: (bytes: Uint8Array) => bytes
}))

class NeverOpeningWebSocket {
  static CONNECTING = 0
  static OPEN = 1
  static CLOSING = 2
  static CLOSED = 3
  readonly CONNECTING = 0
  readonly OPEN = 1
  readonly CLOSING = 2
  readonly CLOSED = 3
  readyState = 0
  onopen: (() => void) | null = null
  onclose: (() => void) | null = null
  onmessage: ((event: { data: unknown }) => void) | null = null
  onerror: (() => void) | null = null
  send(): void {}
  close(): void {
    this.readyState = 3
  }
}

const originalWebSocket = globalThis.WebSocket

afterEach(() => {
  globalThis.WebSocket = originalWebSocket
})

describe('redactedEndpoint', () => {
  it('keeps host:port and drops path/query the old regex would surface', () => {
    expect(redactedEndpoint('wss://desktop.example:7443/runtime?token=super-secret')).toBe(
      'desktop.example:7443'
    )
  })

  it('strips URL userinfo credentials the old regex leaked', () => {
    // Old regex captured `[^/]+` after the scheme, i.e. user:password@host:port.
    expect(redactedEndpoint('wss://user:password@desktop.example:7443/runtime')).toBe(
      'desktop.example:7443'
    )
  })

  it('accepts plain ws:// pairing addresses', () => {
    expect(redactedEndpoint('ws://desktop.example:7443/runtime')).toBe('desktop.example:7443')
  })

  it('falls back to unknown for unparseable endpoints', () => {
    expect(redactedEndpoint('not a url')).toBe('unknown')
  })
})

describe('mobile rpc-client connection logs', () => {
  it.each([
    'wss://desktop.example:7443?token=super-secret',
    'wss://desktop.example:7443#super-secret',
    'wss://user:password@desktop.example:7443?token=super-secret'
  ])('redacts credentials from endpoints without a path: %s', (endpoint) => {
    globalThis.WebSocket = NeverOpeningWebSocket as unknown as typeof WebSocket
    const logs: ConnectionLogEntry[] = []
    const client = connect(endpoint, 'device-token', 'server-key', {
      onLog: (entry) => logs.push(entry)
    })
    try {
      expect(logs[0]?.detail).toBe('desktop.example:7443')
      expect(JSON.stringify(logs)).not.toContain('super-secret')
      expect(JSON.stringify(logs)).not.toContain('password')
    } finally {
      client.close()
    }
  })

  it('never exposes endpoint query credentials', () => {
    globalThis.WebSocket = NeverOpeningWebSocket as unknown as typeof WebSocket
    const logs: ConnectionLogEntry[] = []
    const endpoint = 'wss://desktop.example:7443/runtime?token=super-secret&route=private'

    const client = connect(endpoint, 'device-token', 'server-key', {
      onLog: (entry) => logs.push(entry)
    })

    expect(logs).toContainEqual(
      expect.objectContaining({ message: 'Opening WebSocket', detail: 'desktop.example:7443' })
    )
    expect(JSON.stringify(logs)).not.toContain('super-secret')
    expect(JSON.stringify(logs)).not.toContain('route=private')
    client.close()
  })

  it('does not expose URL credentials in malformed legacy endpoints', () => {
    globalThis.WebSocket = NeverOpeningWebSocket as unknown as typeof WebSocket
    const logs: ConnectionLogEntry[] = []

    const client = connect('wss://user:password@desktop.example:7443/runtime', 'token', 'key', {
      onLog: (entry) => logs.push(entry)
    })

    expect(logs[0]?.detail).toBe('desktop.example:7443')
    expect(JSON.stringify(logs)).not.toContain('password')
    client.close()
  })
})
