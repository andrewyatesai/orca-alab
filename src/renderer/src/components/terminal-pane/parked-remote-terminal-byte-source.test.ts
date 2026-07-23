import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { RuntimeTerminalStreamEndReason } from '@/runtime/runtime-terminal-stream'

const PTY_ID = 'remote:env-1@@terminal-1'

type StreamSubscription = {
  settings: unknown
  ptyId: string
  clientId: string
  watcher: (data: string) => void
  options?: {
    startAtLiveTail?: boolean
    onStreamEnd?: (reason: RuntimeTerminalStreamEndReason) => void
  }
  unsubscribe: ReturnType<typeof vi.fn>
}

const streamSubscriptions: StreamSubscription[] = []
let subscribeFailure: Error | null = null
const subscribeToRuntimeTerminalData = vi.fn(
  async (
    settings: unknown,
    ptyId: string,
    clientId: string,
    watcher: (data: string) => void,
    options?: StreamSubscription['options']
  ) => {
    if (subscribeFailure) {
      throw subscribeFailure
    }
    const unsubscribe = vi.fn()
    streamSubscriptions.push({ settings, ptyId, clientId, watcher, options, unsubscribe })
    return unsubscribe
  }
)

vi.mock('@/runtime/runtime-terminal-stream', async (importOriginal) => ({
  ...(await importOriginal<object>()),
  subscribeToRuntimeTerminalData: (...args: Parameters<typeof subscribeToRuntimeTerminalData>) =>
    subscribeToRuntimeTerminalData(...args)
}))

const callRuntimeRpc = vi.fn()
vi.mock('@/runtime/runtime-rpc-client', async (importOriginal) => ({
  ...(await importOriginal<object>()),
  callRuntimeRpc: (...args: unknown[]) => callRuntimeRpc(...args)
}))

import { createParkedRemoteTerminalByteSource } from './parked-remote-terminal-byte-source'

async function flushAsync(): Promise<void> {
  await vi.advanceTimersByTimeAsync(0)
}

describe('createParkedRemoteTerminalByteSource', () => {
  const onExitConfirmed = vi.fn()

  beforeEach(() => {
    vi.useFakeTimers()
    callRuntimeRpc.mockResolvedValue({ wait: { satisfied: true } })
  })

  afterEach(() => {
    vi.useRealTimers()
    streamSubscriptions.length = 0
    subscribeFailure = null
    vi.clearAllMocks()
  })

  function createSource(
    overrides: Partial<Parameters<typeof createParkedRemoteTerminalByteSource>[0]> = {}
  ): ReturnType<typeof createParkedRemoteTerminalByteSource> {
    return createParkedRemoteTerminalByteSource({
      ptyId: PTY_ID,
      settings: { activeRuntimeEnvironmentId: 'env-active' },
      onExitConfirmed,
      ...overrides
    })
  }

  it('shares one live-tail stream among all subscribers and closes it after the last leaves', async () => {
    const source = createSource()
    const parser = vi.fn()
    const responder = vi.fn()
    const unsubscribeParser = source.subscribeBytes(parser)
    const unsubscribeResponder = source.subscribeBytes(responder)
    await flushAsync()

    expect(subscribeToRuntimeTerminalData).toHaveBeenCalledTimes(1)
    const stream = streamSubscriptions[0]
    expect(stream.ptyId).toBe(PTY_ID)
    // Why live tail: watcher start must never re-fire stale bells from the historical snapshot.
    expect(stream.options?.startAtLiveTail).toBe(true)

    stream.watcher('bell\x07')
    expect(parser).toHaveBeenCalledWith('bell\x07')
    expect(responder).toHaveBeenCalledWith('bell\x07')

    unsubscribeParser()
    expect(stream.unsubscribe).not.toHaveBeenCalled()
    unsubscribeResponder()
    expect(stream.unsubscribe).toHaveBeenCalledTimes(1)
  })

  it('pins the owner environment from the pty id over the active environment', async () => {
    const source = createSource()
    source.subscribeBytes(vi.fn())
    await flushAsync()

    expect(source.runtimeEnvironmentId).toBe('env-1')
    expect(streamSubscriptions[0].settings).toEqual({ activeRuntimeEnvironmentId: 'env-1' })
  })

  it('falls back to the park-time active environment for legacy owner-less remote ids', async () => {
    const source = createSource({ ptyId: 'remote:terminal-1' })
    source.subscribeBytes(vi.fn())
    await flushAsync()

    expect(source.runtimeEnvironmentId).toBe('env-active')
    expect(streamSubscriptions[0].settings).toEqual({ activeRuntimeEnvironmentId: 'env-active' })
  })

  it('reports exit only after the runtime confirms a stream end via terminal.wait', async () => {
    const source = createSource()
    source.subscribeBytes(vi.fn())
    await flushAsync()

    streamSubscriptions[0].options?.onStreamEnd?.('end')
    await flushAsync()

    expect(callRuntimeRpc).toHaveBeenCalledWith(
      { kind: 'environment', environmentId: 'env-1' },
      'terminal.wait',
      { terminal: 'terminal-1', for: 'exit', timeoutMs: 1_000 },
      expect.anything()
    )
    expect(onExitConfirmed).toHaveBeenCalledTimes(1)
    // Confirmed exit stops the source: no resubscribe attempts follow.
    await vi.advanceTimersByTimeAsync(60_000)
    expect(subscribeToRuntimeTerminalData).toHaveBeenCalledTimes(1)
    source.dispose()
  })

  it('treats terminal_gone as exit confirmation', async () => {
    callRuntimeRpc.mockRejectedValue(new Error('terminal_gone'))
    const source = createSource()
    source.subscribeBytes(vi.fn())
    await flushAsync()

    streamSubscriptions[0].options?.onStreamEnd?.('end')
    await flushAsync()

    expect(onExitConfirmed).toHaveBeenCalledTimes(1)
    source.dispose()
  })

  it('resubscribes with backoff after an unconfirmed stream end, without reporting exit', async () => {
    callRuntimeRpc.mockRejectedValue(new Error('timeout'))
    const source = createSource()
    source.subscribeBytes(vi.fn())
    await flushAsync()

    streamSubscriptions[0].options?.onStreamEnd?.('end')
    await flushAsync()
    expect(onExitConfirmed).not.toHaveBeenCalled()
    expect(subscribeToRuntimeTerminalData).toHaveBeenCalledTimes(1)

    await vi.advanceTimersByTimeAsync(1_000)
    expect(subscribeToRuntimeTerminalData).toHaveBeenCalledTimes(2)

    // The next unconfirmed end doubles the delay before the retry.
    streamSubscriptions[1].options?.onStreamEnd?.('end')
    await flushAsync()
    await vi.advanceTimersByTimeAsync(1_000)
    expect(subscribeToRuntimeTerminalData).toHaveBeenCalledTimes(2)
    await vi.advanceTimersByTimeAsync(1_000)
    expect(subscribeToRuntimeTerminalData).toHaveBeenCalledTimes(3)
    expect(onExitConfirmed).not.toHaveBeenCalled()

    // Flowing bytes prove health and reset the backoff to its initial delay.
    streamSubscriptions[2].watcher('healthy')
    streamSubscriptions[2].options?.onStreamEnd?.('end')
    await flushAsync()
    await vi.advanceTimersByTimeAsync(1_000)
    expect(subscribeToRuntimeTerminalData).toHaveBeenCalledTimes(4)
    source.dispose()
  })

  it('resubscribes after routine transport churn without classifying exit', async () => {
    const source = createSource()
    const parser = vi.fn()
    source.subscribeBytes(parser)
    await flushAsync()

    streamSubscriptions[0].options?.onStreamEnd?.('transport-close')
    await vi.advanceTimersByTimeAsync(1_000)

    expect(callRuntimeRpc).not.toHaveBeenCalled()
    expect(onExitConfirmed).not.toHaveBeenCalled()
    expect(subscribeToRuntimeTerminalData).toHaveBeenCalledTimes(2)
    // Existing subscribers keep receiving bytes from the replacement stream.
    streamSubscriptions[1].watcher('after-resubscribe')
    expect(parser).toHaveBeenCalledWith('after-resubscribe')
    source.dispose()
  })

  it('confirms exit when the subscribe attempt itself fails with a gone terminal', async () => {
    subscribeFailure = new Error('terminal_exited')
    const source = createSource()
    source.subscribeBytes(vi.fn())
    await flushAsync()

    expect(onExitConfirmed).toHaveBeenCalledTimes(1)
    source.dispose()
  })

  it('retries a failed subscribe attempt with backoff for recoverable errors', async () => {
    subscribeFailure = new Error('offline')
    const source = createSource()
    source.subscribeBytes(vi.fn())
    await flushAsync()

    expect(onExitConfirmed).not.toHaveBeenCalled()
    subscribeFailure = null
    await vi.advanceTimersByTimeAsync(1_000)
    expect(subscribeToRuntimeTerminalData).toHaveBeenCalledTimes(2)
    expect(streamSubscriptions).toHaveLength(1)
    source.dispose()
  })

  it('dispose cancels pending retries and closes the active stream', async () => {
    const source = createSource()
    source.subscribeBytes(vi.fn())
    await flushAsync()

    source.dispose()
    expect(streamSubscriptions[0].unsubscribe).toHaveBeenCalledTimes(1)

    // A post-dispose stream end must neither classify nor resubscribe.
    streamSubscriptions[0].options?.onStreamEnd?.('end')
    await vi.advanceTimersByTimeAsync(60_000)
    expect(callRuntimeRpc).not.toHaveBeenCalled()
    expect(subscribeToRuntimeTerminalData).toHaveBeenCalledTimes(1)
  })
})
