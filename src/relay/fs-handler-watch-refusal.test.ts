import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest'
import { homedir } from 'node:os'
import { FsHandler } from './fs-handler'
import { RelayContext } from './context'
import type { RelayDispatcher } from './dispatcher'
import { subscribeWithInProcessWatcher } from '../main/ipc/parcel-watcher-in-process-fallback'

const { mockSubscribe } = vi.hoisted(() => ({
  mockSubscribe: vi.fn()
}))

vi.mock('@parcel/watcher', () => ({
  subscribe: mockSubscribe
}))

type WatchRequestContext = { clientId: number; isStale: () => boolean }

function createWatchDispatcher() {
  const requestHandlers = new Map<
    string,
    (params: Record<string, unknown>, context?: WatchRequestContext) => Promise<unknown>
  >()
  const notificationHandlers = new Map<
    string,
    (params: Record<string, unknown>, context?: WatchRequestContext) => void
  >()
  const context: WatchRequestContext = { clientId: 1, isStale: () => false }
  return {
    onRequest: vi.fn(
      (method: string, handler: (params: Record<string, unknown>) => Promise<unknown>) => {
        requestHandlers.set(method, handler)
      }
    ),
    onNotification: vi.fn(
      (method: string, handler: (params: Record<string, unknown>) => void) => {
        notificationHandlers.set(method, handler)
      }
    ),
    notify: vi.fn(),
    notifyClient: vi.fn(),
    onClientDetached: vi.fn(() => () => {}),
    callRequest: (method: string, params: Record<string, unknown>) =>
      requestHandlers.get(method)!(params, context),
    callNotification: (method: string, params: Record<string, unknown>) =>
      notificationHandlers.get(method)!(params, context)
  }
}

describe('fs.watch broad-root refusal (#7948)', () => {
  let dispatcher: ReturnType<typeof createWatchDispatcher>
  let handler: FsHandler

  beforeEach(() => {
    mockSubscribe.mockReset()
    mockSubscribe.mockResolvedValue({ unsubscribe: vi.fn() })
    dispatcher = createWatchDispatcher()
    handler = new FsHandler(dispatcher as unknown as RelayDispatcher, new RelayContext(), {
      dispose: vi.fn(),
      forgetRoot: vi.fn(),
      subscribe: subscribeWithInProcessWatcher
    })
  })

  afterEach(() => {
    handler.dispose()
  })

  it('refuses broad roots (home, ~, /) without subscribing', async () => {
    // Why: a home-rooted recursive watch makes the watcher crawl the whole
    // account tree (container storage, model dirs) and starves the relay.
    await expect(
      dispatcher.callRequest('fs.watch', { rootPath: homedir() })
    ).resolves.toBeUndefined()
    await expect(dispatcher.callRequest('fs.watch', { rootPath: '~' })).resolves.toBeUndefined()
    await expect(dispatcher.callRequest('fs.watch', { rootPath: '/' })).resolves.toBeUndefined()
    expect(mockSubscribe).not.toHaveBeenCalled()
    // No watch state registered — unwatch for those roots is a no-op, not a crash.
    dispatcher.callNotification('fs.unwatch', { rootPath: homedir() })
    await expect(
      dispatcher.callRequest('fs.unwatchAndWait', { rootPath: homedir() })
    ).resolves.toBeUndefined()
  })
})
