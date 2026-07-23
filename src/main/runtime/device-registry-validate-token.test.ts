import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { mkdtempSync, rmSync } from 'node:fs'
import { join } from 'node:path'
import { tmpdir } from 'node:os'
import { DeviceRegistry } from './device-registry'
import * as timingSafe from '../../shared/timing-safe-token-compare'

// Pins the timing-safe token lookup: device tokens are secret bearer
// credentials, so validateToken must compare them in constant time (like the
// runtime-auth path) rather than with a short-circuiting `===` that leaks how
// many leading bytes match via response timing.
describe('DeviceRegistry.validateToken', () => {
  let root: string
  let registry: DeviceRegistry

  beforeEach(() => {
    root = mkdtempSync(join(tmpdir(), 'device-registry-'))
    registry = new DeviceRegistry(root)
  })

  afterEach(() => {
    rmSync(root, { recursive: true, force: true })
  })

  it('returns the matching device for a valid token', () => {
    const entry = registry.addDevice('phone', 'mobile')
    expect(registry.validateToken(entry.token)?.deviceId).toBe(entry.deviceId)
  })

  it('returns null for an unknown token', () => {
    registry.addDevice('phone', 'mobile')
    expect(registry.validateToken('deadbeef')).toBeNull()
  })

  it('routes every comparison through the constant-time helper (no `===` short-circuit)', () => {
    const spy = vi.spyOn(timingSafe, 'timingSafeTokenCompare')
    const a = registry.addDevice('a', 'mobile')
    registry.addDevice('b', 'mobile')
    registry.addDevice('c', 'mobile')

    spy.mockClear()
    const result = registry.validateToken(a.token)

    expect(result?.deviceId).toBe(a.deviceId)
    // No early return on first match: all three devices are compared so the
    // per-request work does not reveal which device (or how many bytes) matched.
    expect(spy).toHaveBeenCalledTimes(3)
    spy.mockRestore()
  })
})
