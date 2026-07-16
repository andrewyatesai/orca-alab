// The fork's daemon protocol namespace split (staging-launch audit G3-0/G3-1/
// G3-3): the fork reserves 1000+ so its daemon endpoints are disjoint from any
// public Orca install, and the public Node daemon's version (18) is listed as
// a previous version so live public sessions migrate via the legacy adapter.
import { describe, expect, it } from 'vitest'
import { getDaemonPidPath, getDaemonSocketPath, getDaemonTokenPath } from './daemon-spawner'
import { PREVIOUS_DAEMON_PROTOCOL_VERSIONS, PROTOCOL_VERSION } from './types'

describe('fork daemon protocol namespace', () => {
  it('pins the fork protocol version to the 1000+ namespace', () => {
    expect(PROTOCOL_VERSION).toBe(1019)
  })

  it('embeds v1019 in the default socket/token/pid endpoint names', () => {
    const runtimeDir = '/fake/daemon'
    // Why literal 1019 (not the constant): the whole point is that a public
    // build's endpoints (daemon-v18.*) can never collide with the fork's. A
    // symbolic assertion would keep passing if the namespace regressed to 18.
    expect(getDaemonSocketPath(runtimeDir)).toContain('v1019')
    if (process.platform !== 'win32') {
      expect(getDaemonSocketPath(runtimeDir)).toBe('/fake/daemon/daemon-v1019.sock')
    }
    expect(getDaemonTokenPath(runtimeDir).endsWith('daemon-v1019.token')).toBe(true)
    expect(getDaemonPidPath(runtimeDir).endsWith('daemon-v1019.pid')).toBe(true)
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

  it('lists the previous fork version 1018 as a previous version', () => {
    // Why: a fork daemon preserved across the 1019 (subscriber-role) app
    // update lives at daemon-v1018.* and must keep its sessions via the same
    // legacy-adapter path — the TS side must not require 1019.
    expect(PREVIOUS_DAEMON_PROTOCOL_VERSIONS).toContain(1018)
  })

  it('never lists the current version as previous', () => {
    // Why: pty-management routes sessions to adapters by protocolVersion and
    // relies on the current version being distinct from every previous one.
    expect(PREVIOUS_DAEMON_PROTOCOL_VERSIONS).not.toContain(PROTOCOL_VERSION)
  })
})
