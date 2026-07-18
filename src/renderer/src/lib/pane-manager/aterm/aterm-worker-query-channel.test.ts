import { afterEach, describe, expect, it, vi } from 'vitest'
import { createAtermWorkerQueryChannel } from './aterm-worker-query-channel'
import type { AtermWorkerPaneCommand } from './aterm-render-worker-protocol'

afterEach(() => {
  vi.useRealTimers()
})

type QueryCommand = Extract<AtermWorkerPaneCommand, { type: 'query' }>

function captureQueries(): {
  channel: ReturnType<typeof createAtermWorkerQueryChannel>
  posted: QueryCommand[]
} {
  const posted: QueryCommand[] = []
  const channel = createAtermWorkerQueryChannel((cmd) => {
    if (cmd.type === 'query') {
      posted.push(cmd)
    }
  })
  return { channel, posted }
}

// The settle fence's boolean discriminant is the load-bearing correctness contract:
// the replay guard may only treat a TRUE (real 'flush' queryResult) resolution as
// parse-certified. A time-based (timeout) or dispose resolution must be FALSE, so a
// worker that is alive-but->5s-behind can't trick the guard into releasing before it
// parses replayed query bytes (DA1/CPR/OSC) — which would leak them as stray input.
describe('createAtermWorkerQueryChannel settleAsync discriminant', () => {
  it('resolves TRUE only when the real flush queryResult arrives (parse-certified)', async () => {
    const { channel, posted } = captureQueries()
    const settled = channel.settleAsync()

    // The worker answers the flush query id with a real queryResult (value irrelevant).
    const flush = posted.find((cmd) => cmd.kind === 'flush')
    expect(flush).toBeDefined()
    channel.resolve(flush!.id, null)

    expect(await settled).toBe(true)
  })

  it('resolves FALSE on the fence timeout (worker alive-but-behind, not certified)', async () => {
    vi.useFakeTimers()
    const { channel } = captureQueries()
    const settled = channel.settleAsync()

    // QUERY_TIMEOUT_MS elapses with no real reply — the worker is merely behind.
    await vi.advanceTimersByTimeAsync(5_000)

    expect(await settled).toBe(false)
  })

  it('resolves FALSE when the channel is disposed before a reply (terminated worker)', async () => {
    const { channel } = captureQueries()
    const settled = channel.settleAsync()

    channel.dispose()

    expect(await settled).toBe(false)
  })

  it('resolves FALSE immediately for a fence posted after dispose (no worker to reply)', async () => {
    const { channel } = captureQueries()
    channel.dispose()

    expect(await channel.settleAsync()).toBe(false)
  })

  it('keeps returning the reply payload for content queries (serialize) unaffected', async () => {
    const { channel, posted } = captureQueries()
    const serialized = channel.serializeAsync()

    const query = posted.find((cmd) => cmd.kind === 'serialize')
    expect(query).toBeDefined()
    channel.resolve(query!.id, 'replayable-ansi')

    expect(await serialized).toBe('replayable-ansi')
  })
})
