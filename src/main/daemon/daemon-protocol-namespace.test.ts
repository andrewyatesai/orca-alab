// The fork's daemon protocol namespace split (staging-launch audit G3-0/G3-1/
// G3-3): the fork reserves 1000+ so its daemon endpoints are disjoint from any
// public Orca install, and the public Node daemon's version (18) is listed as
// a previous version so live public sessions migrate via the legacy adapter.
import { describe, expect, it } from 'vitest'
import { getDaemonPidPath, getDaemonSocketPath, getDaemonTokenPath } from './daemon-spawner'
import { PREVIOUS_DAEMON_PROTOCOL_VERSIONS, PROTOCOL_VERSION } from './types'

describe('fork daemon protocol namespace', () => {
  it('pins the fork protocol version to the 1000+ namespace', () => {
    expect(PROTOCOL_VERSION).toBe(1020)
  })

  it('embeds v1020 in the default socket/token/pid endpoint names', () => {
    const runtimeDir = '/fake/daemon'
    // Why literal 1020 (not the constant): the whole point is that a public
    // build's endpoints (daemon-v18.*) can never collide with the fork's. A
    // symbolic assertion would keep passing if the namespace regressed to 18.
    expect(getDaemonSocketPath(runtimeDir)).toContain('v1020')
    if (process.platform !== 'win32') {
      expect(getDaemonSocketPath(runtimeDir)).toBe('/fake/daemon/daemon-v1020.sock')
    }
    expect(getDaemonTokenPath(runtimeDir).endsWith('daemon-v1020.token')).toBe(true)
    expect(getDaemonPidPath(runtimeDir).endsWith('daemon-v1020.pid')).toBe(true)
  })

  it('is disjoint from the public endpoint namespace', () => {
    const socketPath = getDaemonSocketPath('/fake/daemon')
    expect(socketPath).not.toContain('v18.')
    expect(socketPath).not.toMatch(/v18\b/)
  })

  it('lists the public Node daemon version 18 as a previous version', () => {
    // Why: a live public daemon with running agent sessions must be attached
    // through the legacy-adapter path, not killed or impersonated.
    expect(PREVIOUS_DAEMON_PROTOCOL_VERSIONS).toContain(18)
  })

  it('lists the previous fork versions 1018 and 1019 as previous versions', () => {
    // Why: a fork daemon preserved across an app update to 1020 (the binary
    // stream plane) lives at daemon-v1018.* / daemon-v1019.* and must keep its
    // sessions via the legacy-adapter path — the TS side must not require 1020.
    expect(PREVIOUS_DAEMON_PROTOCOL_VERSIONS).toContain(1018)
    expect(PREVIOUS_DAEMON_PROTOCOL_VERSIONS).toContain(1019)
  })

  it('never lists the current version as previous', () => {
    // Why: pty-management routes sessions to adapters by protocolVersion and
    // relies on the current version being distinct from every previous one.
    expect(PREVIOUS_DAEMON_PROTOCOL_VERSIONS).not.toContain(PROTOCOL_VERSION)
  })
})
