import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { SshTarget } from '../../shared/ssh-types'

type MockStatus = 'connecting' | 'connected' | 'disconnected' | 'reconnecting'

const mockState = vi.hoisted(() => ({
  connectResults: [] as Promise<void>[],
  instances: [] as {
    connect: ReturnType<typeof vi.fn>
    disconnect: ReturnType<typeof vi.fn>
    status: 'connecting' | 'connected' | 'disconnected' | 'reconnecting'
    target?: { host?: string; port?: number; username?: string }
  }[]
}))

vi.mock('./ssh-connection', () => ({
  SshConnection: class MockSshConnection {
    status: MockStatus = 'connecting'
    target: { host?: string; port?: number; username?: string } | undefined
    connect = vi.fn(async () => {
      await (mockState.connectResults.shift() ?? Promise.resolve())
      this.status = 'connected'
    })
    disconnect = vi.fn(async () => {
      this.status = 'disconnected'
    })

    constructor(target?: { host?: string; port?: number; username?: string }) {
      this.target = target
      mockState.instances.push(this)
    }

    getState(): { status: MockStatus } {
      return { status: this.status }
    }

    matchesTarget(target: { host?: string; port?: number; username?: string }): boolean {
      return (
        this.target?.host === target.host &&
        this.target?.port === target.port &&
        this.target?.username === target.username
      )
    }

    setCallbacks(): void {}
  }
}))

import { SshConnectionManager } from './ssh-connection-manager'

const target = {
  id: 'target-1',
  label: 'Target 1',
  host: 'example.test',
  port: 22,
  username: 'demo',
  source: 'manual'
} as SshTarget

describe('SshConnectionManager', () => {
  beforeEach(() => {
    mockState.connectResults.length = 0
    mockState.instances.length = 0
  })

  it('lets disconnect start a new connect before the cancelled attempt settles', async () => {
    let rejectFirst!: (error: Error) => void
    mockState.connectResults.push(
      new Promise<void>((_resolve, reject) => {
        rejectFirst = reject
      }),
      Promise.resolve()
    )
    const manager = new SshConnectionManager({
      onStateChange: vi.fn()
    })

    const firstConnect = manager.connect(target)
    await manager.disconnect(target.id)
    const secondConnection = await manager.connect(target)
    rejectFirst(new Error('cancelled'))

    await expect(firstConnect).rejects.toThrow('cancelled')
    expect(mockState.instances).toHaveLength(2)
    expect(manager.getConnection(target.id)).toBe(secondConnection)
  })

  it('does not resurrect a connection when disconnect races a reconnecting connect', async () => {
    const manager = new SshConnectionManager({ onStateChange: vi.fn() })
    await manager.connect(target)
    // Simulate an auto-reconnect in progress so connect() skips the connected
    // early-return and falls through to the disconnect+rebuild path.
    mockState.instances[0].status = 'reconnecting'

    const connectPromise = manager.connect(target).catch(() => undefined)
    const disconnectPromise = manager.disconnect(target.id)
    await Promise.all([connectPromise, disconnectPromise])

    // The user's disconnect must win — no live connection may linger under the id.
    expect(manager.getConnection(target.id)).toBeUndefined()
    expect(mockState.instances).toHaveLength(1)
  })

  it('rebuilds instead of returning a pooled connection bound to a stale endpoint', async () => {
    const manager = new SshConnectionManager({ onStateChange: vi.fn() })
    const first = await manager.connect(target)

    // Same id, edited host (as updateTarget mutates in place): must not reuse.
    const second = await manager.connect({ ...target, host: 'new.host' } as SshTarget)

    expect(second).not.toBe(first)
    expect(mockState.instances).toHaveLength(2)
    expect(manager.getConnection(target.id)).toBe(second)
  })
})
