import { describe, expect, it } from 'vitest'
import { PREVIOUS_DAEMON_PROTOCOL_VERSIONS, PROTOCOL_VERSION } from './types'

describe('foreground-confirmation daemon protocol', () => {
  it('rejects daemons from before the fresh-confirmation RPC', () => {
    // Fork: PROTOCOL_VERSION lives in the 1000+ fork namespace, not upstream's public 24.
    expect(PROTOCOL_VERSION).toBeGreaterThan(19)
    expect(PREVIOUS_DAEMON_PROTOCOL_VERSIONS).toContain(19)
    expect(PREVIOUS_DAEMON_PROTOCOL_VERSIONS).toContain(22)
    expect(PREVIOUS_DAEMON_PROTOCOL_VERSIONS).toContain(23)
    expect(PREVIOUS_DAEMON_PROTOCOL_VERSIONS).toContain(24)
  })
})
