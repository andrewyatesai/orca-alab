import { describe, expect, it } from 'vitest'
import { MobileRelayRpcStreams } from './mobile-relay-rpc-streams'
import type { RpcResponse } from './types'

type Frame = { id: string; method: string; params?: unknown }

function readyResponse(id: string, subscriptionId: string): RpcResponse {
  return {
    id,
    ok: true,
    streaming: true,
    result: { subscriptionId },
    _meta: { runtimeId: 'test-runtime' }
  }
}

function createHarness(options?: { deferConnect?: boolean }) {
  const frames: Frame[] = []
  let counter = 0
  let resolveConnected: () => void = () => {}
  const connected = new Promise<void>((resolve) => {
    resolveConnected = resolve
  })
  const streams = new MobileRelayRpcStreams({
    nextId: () => `id-${++counter}`,
    sendFrame: (frame) => {
      frames.push(frame)
      return true
    },
    waitForConnected: () => (options?.deferConnect ? connected : Promise.resolve())
  })
  return { streams, frames, resolveConnected }
}

const flush = () => new Promise((resolve) => setTimeout(resolve, 0))

describe('MobileRelayRpcStreams cancel teardown', () => {
  it('unsubscribes session.tabs on cancel echoing the subscribe id as subscriptionId', async () => {
    const { streams, frames } = createHarness()
    const dispose = streams.subscribe('session.tabs.subscribe', { worktree: 'id:wt-1' }, () => {})
    await flush()
    // The subscribe frame's id is the host's per-subscriber cleanup key.
    const subscribeId = frames.find((f) => f.method === 'session.tabs.subscribe')?.id
    expect(subscribeId).toBeDefined()

    dispose()

    const unsub = frames.find((f) => f.method === 'session.tabs.unsubscribe')
    expect(unsub).toBeDefined()
    // Why: a subscriptionId-less unsubscribe is a PREFIX wipe on the host; the
    // targeted teardown must carry the original subscribe id.
    expect(unsub?.params).toEqual({ worktree: 'id:wt-1', subscriptionId: subscribeId })
  })

  it('canceling one session.tabs subscriber does not broadcast a prefix-wipe teardown for a sibling', async () => {
    const { streams, frames } = createHarness()
    // Two mounted screens subscribe to the same worktree on one socket.
    const disposeA = streams.subscribe('session.tabs.subscribe', { worktree: 'id:wt-1' }, () => {})
    streams.subscribe('session.tabs.subscribe', { worktree: 'id:wt-1' }, () => {})
    await flush()
    const subscribeIds = frames
      .filter((f) => f.method === 'session.tabs.subscribe')
      .map((f) => f.id)
    expect(subscribeIds).toHaveLength(2)
    const [idA] = subscribeIds

    disposeA()

    const unsubs = frames.filter((f) => f.method === 'session.tabs.unsubscribe')
    expect(unsubs).toHaveLength(1)
    // Targets only subscriber A's key; a subscriptionId-less frame would be a
    // prefix wipe on the host that also silently tears down subscriber B.
    expect(unsubs[0]?.params).toEqual({ worktree: 'id:wt-1', subscriptionId: idA })
  })

  it('runtime.clientEvents: cancel before ready then unsubscribes once when ready lands', async () => {
    const { streams, frames } = createHarness()
    const dispose = streams.subscribe('runtime.clientEvents.subscribe', null, () => {})
    await flush()
    const subscribeFrame = frames.find((f) => f.method === 'runtime.clientEvents.subscribe')
    expect(subscribeFrame).toBeDefined()

    dispose()
    // No subscriptionId known yet -> tombstone, nothing unsubscribed.
    expect(frames.some((f) => f.method === 'runtime.clientEvents.unsubscribe')).toBe(false)

    // Late ready delivers the subscriptionId -> unsubscribe fires exactly once.
    streams.handleResponse(readyResponse(subscribeFrame!.id, 'sub-runtime'))
    const unsubs = frames.filter((f) => f.method === 'runtime.clientEvents.unsubscribe')
    expect(unsubs).toHaveLength(1)
    expect(unsubs[0]?.params).toEqual({ subscriptionId: 'sub-runtime' })

    // A second stray response must not re-emit an unsubscribe.
    streams.handleResponse(readyResponse(subscribeFrame!.id, 'sub-runtime'))
    expect(frames.filter((f) => f.method === 'runtime.clientEvents.unsubscribe')).toHaveLength(1)
  })

  it('browser.screencast cancel emits browser.screencast.unsubscribe (not a re-subscribe)', async () => {
    const { streams, frames } = createHarness()
    const dispose = streams.subscribe('browser.screencast', { pageId: 'p1' }, () => {})
    await flush()
    const subscribeFrame = frames.find((f) => f.method === 'browser.screencast')
    streams.handleResponse(readyResponse(subscribeFrame!.id, 'sub-cast'))

    dispose()

    expect(frames.some((f) => f.method === 'browser.screencast.unsubscribe')).toBe(true)
    // Must never re-emit the plain subscribe method as its own "unsubscribe".
    expect(frames.filter((f) => f.method === 'browser.screencast')).toHaveLength(1)
  })

  it('terminal.subscribe cancel still emits terminal.unsubscribe', async () => {
    const { streams, frames } = createHarness()
    const dispose = streams.subscribe('terminal.subscribe', { terminal: 't1' }, () => {})
    await flush()

    dispose()

    const unsub = frames.find((f) => f.method === 'terminal.unsubscribe')
    expect(unsub).toBeDefined()
    expect(unsub?.params).toMatchObject({ subscriptionId: 't1' })
  })

  it('cancel before the subscribe frame is sent emits no unsubscribe and no subscribe', async () => {
    const { streams, frames, resolveConnected } = createHarness({ deferConnect: true })
    const dispose = streams.subscribe('runtime.clientEvents.subscribe', null, () => {})
    await flush()
    // Not connected yet: nothing sent.
    expect(frames).toHaveLength(0)

    dispose()
    // Now allow the connection to resolve; the cancelled stream must not send.
    resolveConnected()
    await flush()

    expect(frames).toHaveLength(0)
  })
})
