import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  refitAndRefreshAllTerminalPanes,
  registerLivePaneManager,
  unregisterLivePaneManager
} from './pane-manager-registry'

type RegisteredManager = {
  fitAllPanes?: () => void
  refreshAllPanes?: () => void
}

describe('pane manager registry', () => {
  // Why: the registry is module-global; unregister in afterEach so a failed
  // assertion cannot leak managers into later tests.
  const registeredManagers: RegisteredManager[] = []

  function register(manager: RegisteredManager): RegisteredManager {
    registerLivePaneManager(manager)
    registeredManagers.push(manager)
    return manager
  }

  afterEach(() => {
    for (const manager of registeredManagers.splice(0)) {
      unregisterLivePaneManager(manager)
    }
  })

  it('fits and refreshes every registered manager', () => {
    const first = register({ fitAllPanes: vi.fn(), refreshAllPanes: vi.fn() })
    const second = register({ fitAllPanes: vi.fn(), refreshAllPanes: vi.fn() })

    refitAndRefreshAllTerminalPanes()

    expect(first.fitAllPanes).toHaveBeenCalledTimes(1)
    expect(first.refreshAllPanes).toHaveBeenCalledTimes(1)
    expect(second.fitAllPanes).toHaveBeenCalledTimes(1)
    expect(second.refreshAllPanes).toHaveBeenCalledTimes(1)
  })

  it('stops refitting managers after they unregister', () => {
    const manager = register({ fitAllPanes: vi.fn(), refreshAllPanes: vi.fn() })
    unregisterLivePaneManager(manager)
    registeredManagers.splice(registeredManagers.indexOf(manager), 1)

    refitAndRefreshAllTerminalPanes()

    expect(manager.fitAllPanes).not.toHaveBeenCalled()
  })

  it('continues refitting later managers when one manager throws', () => {
    const broken = register({
      fitAllPanes: vi.fn(() => {
        throw new Error('pane disposed')
      }),
      refreshAllPanes: vi.fn()
    })
    const healthy = register({ fitAllPanes: vi.fn(), refreshAllPanes: vi.fn() })

    expect(() => refitAndRefreshAllTerminalPanes()).not.toThrow()

    expect(broken.fitAllPanes).toHaveBeenCalledTimes(1)
    expect(broken.refreshAllPanes).not.toHaveBeenCalled()
    expect(healthy.fitAllPanes).toHaveBeenCalledTimes(1)
    expect(healthy.refreshAllPanes).toHaveBeenCalledTimes(1)
  })
})
