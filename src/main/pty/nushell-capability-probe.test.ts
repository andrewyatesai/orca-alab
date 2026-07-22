import { afterEach, describe, expect, it } from 'vitest'
import {
  __resetNushellCapabilityProbeCache,
  __seedNushellIntegrationSupport,
  getCachedNushellIntegrationSupport,
  nushellVersionSupportsIntegration,
  probeNushellIntegrationSupport
} from './nushell-capability-probe'

afterEach(() => {
  __resetNushellCapabilityProbeCache()
})

describe('nushellVersionSupportsIntegration', () => {
  it('accepts versions at and above the 0.96.0 floor', () => {
    expect(nushellVersionSupportsIntegration('0.96.0')).toBe(true)
    expect(nushellVersionSupportsIntegration('0.104.1\n')).toBe(true)
    expect(nushellVersionSupportsIntegration('1.0.0')).toBe(true)
  })

  it('rejects versions below the floor and unparseable output', () => {
    expect(nushellVersionSupportsIntegration('0.95.0')).toBe(false)
    expect(nushellVersionSupportsIntegration('0.9.1')).toBe(false)
    expect(nushellVersionSupportsIntegration('')).toBe(false)
    expect(nushellVersionSupportsIntegration('nu: command not found')).toBe(false)
  })

  // Why: Critic note 3 — only the leading numeric token may be compared, or a future suffixed line silently fails the gate.
  it('strips trailing annotations from the version line', () => {
    expect(nushellVersionSupportsIntegration('0.104.0 (abc1234 2026-01-01)')).toBe(true)
    expect(nushellVersionSupportsIntegration('0.95.0 (abc1234)')).toBe(false)
  })
})

describe('probeNushellIntegrationSupport', () => {
  it('cold cache degrades the sync read and upgrades after the probe resolves', async () => {
    expect(getCachedNushellIntegrationSupport('/opt/nu')).toBeUndefined()
    const supported = await probeNushellIntegrationSupport('/opt/nu', {
      runVersionCommand: async () => '0.104.0'
    })
    expect(supported).toBe(true)
    expect(getCachedNushellIntegrationSupport('/opt/nu')).toBe(true)
  })

  it('caches a below-floor answer as false', async () => {
    await probeNushellIntegrationSupport('/opt/old-nu', {
      runVersionCommand: async () => '0.90.1'
    })
    expect(getCachedNushellIntegrationSupport('/opt/old-nu')).toBe(false)
  })

  it('treats a failing --version spawn as unsupported', async () => {
    await probeNushellIntegrationSupport('/opt/broken-nu', {
      runVersionCommand: async () => {
        throw new Error('ENOENT')
      }
    })
    expect(getCachedNushellIntegrationSupport('/opt/broken-nu')).toBe(false)
  })

  it('coalesces concurrent probes onto one version command', async () => {
    let runs = 0
    let release: (value: string) => void = () => {}
    const gate = new Promise<string>((resolve) => {
      release = resolve
    })
    const run = (): Promise<string> => {
      runs++
      return gate
    }
    const first = probeNushellIntegrationSupport('/opt/nu', { runVersionCommand: run })
    const second = probeNushellIntegrationSupport('/opt/nu', { runVersionCommand: run })
    release('0.96.0')
    expect(await first).toBe(true)
    expect(await second).toBe(true)
    expect(runs).toBe(1)
  })

  it('does not re-run a resolved probe', async () => {
    let runs = 0
    const run = async (): Promise<string> => {
      runs++
      return '0.96.0'
    }
    await probeNushellIntegrationSupport('/opt/nu', { runVersionCommand: run })
    await probeNushellIntegrationSupport('/opt/nu', { runVersionCommand: run })
    expect(runs).toBe(1)
  })

  it('isolates cache entries per executable path', async () => {
    await probeNushellIntegrationSupport('/opt/new-nu', {
      runVersionCommand: async () => '0.104.0'
    })
    await probeNushellIntegrationSupport('/opt/old-nu', {
      runVersionCommand: async () => '0.90.0'
    })
    expect(getCachedNushellIntegrationSupport('/opt/new-nu')).toBe(true)
    expect(getCachedNushellIntegrationSupport('/opt/old-nu')).toBe(false)
    expect(getCachedNushellIntegrationSupport('/opt/other-nu')).toBeUndefined()
  })

  it('supports test seeding for spawn-time reads', () => {
    __seedNushellIntegrationSupport('/opt/nu', true)
    expect(getCachedNushellIntegrationSupport('/opt/nu')).toBe(true)
  })
})
