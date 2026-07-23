/**
 * Regression (#10052): a throwing client-event subscriber must not abort the
 * emitting operation. A synchronous throw once escaped the fan-out and, when the
 * emit ran under a held per-worktree terminal mutation, leaked the lock and
 * wedged that worktree's sleep until restart. Isolation must (a) still deliver to
 * every other listener and (b) let the emitting call return normally.
 */
import { describe, it, expect, vi, afterEach } from 'vitest'
import { OrcaRuntimeService } from './orca-runtime'

describe('emitClientEvent listener isolation (#10052)', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('keeps delivering to later listeners and returns normally when one throws', () => {
    // Why: swallow the intentional console.error the isolating helper logs.
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {})
    const runtime = new OrcaRuntimeService(null)

    const throwing = vi.fn(() => {
      throw new Error('subscriber boom')
    })
    const survivor = vi.fn()
    runtime.onClientEvent(throwing)
    runtime.onClientEvent(survivor)

    // notifySshStateChanged is a public entry point onto the client-event stream.
    expect(() => runtime.notifySshStateChanged('target-1', 'connected' as never)).not.toThrow()

    expect(throwing).toHaveBeenCalledTimes(1)
    expect(survivor).toHaveBeenCalledTimes(1)
    expect(survivor).toHaveBeenCalledWith(
      expect.objectContaining({ type: 'sshStateChanged', targetId: 'target-1' })
    )
    expect(errorSpy).toHaveBeenCalled()
  })
})
