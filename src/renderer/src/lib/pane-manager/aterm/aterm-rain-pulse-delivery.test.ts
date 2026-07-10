/** @vitest-environment happy-dom */
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { PaneManager } from '../pane-manager'
import { registerTabPaneManager, unregisterTabPaneManager } from '../pane-manager-registry'
import {
  deliverAtermRainPulse,
  flushPendingAtermRainPulsesAtControllerAttach
} from './aterm-rain-pulse-delivery'

const managers: { tabId: string; manager: PaneManager }[] = []

function register(tabId: string, manager: PaneManager): void {
  registerTabPaneManager(tabId, manager)
  managers.push({ tabId, manager })
}

afterEach(() => {
  for (const { tabId, manager } of managers.splice(0)) {
    unregisterTabPaneManager(tabId, manager)
  }
})

describe('aterm semantic pulse delivery', () => {
  it('targets the exact durable pane key and forwards no content', () => {
    const noteMatrixRainPulse = vi.fn()
    const leafId = '123e4567-e89b-42d3-a456-426614174000'
    const manager = {
      getPanes: () => [
        {
          leafId,
          atermController: { noteMatrixRainPulse }
        }
      ]
    } as unknown as PaneManager
    register('tab-a', manager)

    expect(deliverAtermRainPulse(`tab-a:${leafId}`, { signal: 'modify', weight: 5 })).toBe(true)
    expect(noteMatrixRainPulse).toHaveBeenCalledWith({ signal: 'modify', weight: 5 })
    expect(deliverAtermRainPulse('tab-a:legacy-numeric-id', { signal: 'execute', weight: 6 })).toBe(
      false
    )
  })

  it('keeps a replacement manager registered when the stale lifecycle cleans up', () => {
    const leafId = '123e4567-e89b-42d3-a456-426614174001'
    const stale = { getPanes: () => [] } as unknown as PaneManager
    const noteMatrixRainPulse = vi.fn()
    const current = {
      getPanes: () => [{ leafId, atermController: { noteMatrixRainPulse } }]
    } as unknown as PaneManager
    register('tab-a', stale)
    register('tab-a', current)

    unregisterTabPaneManager('tab-a', stale)

    expect(deliverAtermRainPulse(`tab-a:${leafId}`, { signal: 'inspect', weight: 2 })).toBe(true)
    expect(noteMatrixRainPulse).toHaveBeenCalledTimes(1)
  })

  it('flushes an absent-pane latch on registration with turn then strongest semantics', () => {
    const leafId = '123e4567-e89b-42d3-a456-426614174010'
    const paneKey = `tab-a:${leafId}`
    const noteMatrixRainPulse = vi.fn()

    expect(deliverAtermRainPulse(paneKey, { signal: 'turn_start', weight: 4 })).toBe(false)
    expect(deliverAtermRainPulse(paneKey, { signal: 'inspect', weight: 3 })).toBe(false)
    expect(deliverAtermRainPulse(paneKey, { signal: 'failure', weight: 8 })).toBe(false)
    expect(deliverAtermRainPulse(paneKey, { signal: 'assistant', weight: 2 })).toBe(false)

    register('tab-a', {
      getPanes: () => [{ leafId, atermController: { noteMatrixRainPulse } }]
    } as unknown as PaneManager)

    expect(noteMatrixRainPulse.mock.calls).toEqual([
      [{ signal: 'turn_start', weight: 4 }],
      [{ signal: 'failure', weight: 8 }]
    ])

    // Successful flush consumes the latch: an overlapping/remounted manager
    // must not replay old work.
    const replacementPulse = vi.fn()
    register('tab-a', {
      getPanes: () => [{ leafId, atermController: { noteMatrixRainPulse: replacementPulse } }]
    } as unknown as PaneManager)
    expect(replacementPulse).not.toHaveBeenCalled()
  })

  it('flushes at the exact async controller attach edge', () => {
    const leafId = '123e4567-e89b-42d3-a456-426614174011'
    const noteMatrixRainPulse = vi.fn()
    const controller = { noteMatrixRainPulse }
    let attached: typeof controller | null = null
    register('tab-a', {
      getPanes: () => [{ leafId, atermController: attached }]
    } as unknown as PaneManager)

    expect(deliverAtermRainPulse(`tab-a:${leafId}`, { signal: 'network', weight: 5 })).toBe(false)
    attached = controller
    flushPendingAtermRainPulsesAtControllerAttach(leafId, controller)

    expect(noteMatrixRainPulse).toHaveBeenCalledWith({ signal: 'network', weight: 5 })
  })

  it('delivers to overlapping managers and deduplicates a shared controller identity', () => {
    const leafId = '123e4567-e89b-42d3-a456-426614174002'
    const firstController = { noteMatrixRainPulse: vi.fn() }
    const secondController = { noteMatrixRainPulse: vi.fn() }
    const manager = (controller: typeof firstController): PaneManager =>
      ({ getPanes: () => [{ leafId, atermController: controller }] }) as unknown as PaneManager
    register('tab-a', manager(firstController))
    register('tab-a', manager(secondController))
    register('tab-a', manager(firstController))

    expect(deliverAtermRainPulse(`tab-a:${leafId}`, { signal: 'network', weight: 4 })).toBe(true)
    expect(firstController.noteMatrixRainPulse).toHaveBeenCalledTimes(1)
    expect(secondController.noteMatrixRainPulse).toHaveBeenCalledTimes(1)
  })

  it('isolates stale and tearing-down managers from the accepted status IPC path', () => {
    const leafId = '123e4567-e89b-42d3-a456-426614174003'
    const throwingController = {
      noteMatrixRainPulse: vi.fn(() => {
        throw new Error('disposed engine')
      })
    }
    const healthyController = { noteMatrixRainPulse: vi.fn() }
    register('tab-a', {
      getPanes: () => [{ leafId, atermController: {} }]
    } as unknown as PaneManager)
    register('tab-a', {
      getPanes: () => {
        throw new Error('tearing down')
      }
    } as unknown as PaneManager)
    register('tab-a', {
      getPanes: () => [{ leafId, atermController: throwingController }]
    } as unknown as PaneManager)
    register('tab-a', {
      getPanes: () => [{ leafId, atermController: healthyController }]
    } as unknown as PaneManager)

    expect(() =>
      deliverAtermRainPulse(`tab-a:${leafId}`, { signal: 'success', weight: 7 })
    ).not.toThrow()
    expect(healthyController.noteMatrixRainPulse).toHaveBeenCalledTimes(1)
  })

  it('bounds missing-pane identities and evicts the least recently active slot', () => {
    const leafId = (index: number): string =>
      `00000000-0000-4000-8000-${index.toString(16).padStart(12, '0')}`
    for (let index = 0; index < 65; index++) {
      expect(
        deliverAtermRainPulse(`tab-cap:${leafId(index)}`, { signal: 'assistant', weight: 2 })
      ).toBe(false)
    }

    const oldestPulse = vi.fn()
    register('tab-cap', {
      getPanes: () => [{ leafId: leafId(0), atermController: { noteMatrixRainPulse: oldestPulse } }]
    } as unknown as PaneManager)
    expect(oldestPulse).not.toHaveBeenCalled()

    const newestPulse = vi.fn()
    register('tab-cap', {
      getPanes: () => [
        { leafId: leafId(64), atermController: { noteMatrixRainPulse: newestPulse } }
      ]
    } as unknown as PaneManager)
    expect(newestPulse).toHaveBeenCalledTimes(1)

    // Consume the remaining retained slots so this test also proves successful
    // delivery cleans up bounded state instead of replaying it later.
    const cleanupPulse = vi.fn()
    register('tab-cap', {
      getPanes: () =>
        Array.from({ length: 63 }, (_, index) => ({
          leafId: leafId(index + 1),
          atermController: { noteMatrixRainPulse: cleanupPulse }
        }))
    } as unknown as PaneManager)
    expect(cleanupPulse).toHaveBeenCalledTimes(63)
  })

  it('retains an undelivered latch across a zero-manager remount gap', () => {
    const leafId = '123e4567-e89b-42d3-a456-426614174012'
    const empty = { getPanes: () => [] } as unknown as PaneManager
    register('tab-clean', empty)
    expect(deliverAtermRainPulse(`tab-clean:${leafId}`, { signal: 'modify', weight: 5 })).toBe(
      false
    )

    unregisterTabPaneManager('tab-clean', empty)
    const noteMatrixRainPulse = vi.fn()
    register('tab-clean', {
      getPanes: () => [{ leafId, atermController: { noteMatrixRainPulse } }]
    } as unknown as PaneManager)

    expect(noteMatrixRainPulse).toHaveBeenCalledWith({ signal: 'modify', weight: 5 })
  })
})
