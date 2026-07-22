import { afterEach, describe, expect, it, vi } from 'vitest'
import { createAtermWorkerQueryChannel } from './aterm-worker-query-channel'
import {
  SEARCH_FIND_FLAG_CASE_SENSITIVE,
  SEARCH_FIND_FLAG_REGEX
} from './aterm-worker-search'
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

// P1: find rides the id-correlated channel so the monotonic query id doubles as the
// request GENERATION — a newer find cancels the older's promise instantly (superseded
// queries never block newer ones) and the older's late reply is discarded by id.
describe('createAtermWorkerQueryChannel searchFindAsync', () => {
  it('posts the query text + flag bits and resolves the parsed result', async () => {
    const { channel, posted } = captureQueries()
    const result = channel.searchFindAsync('needle', true, true)

    const query = posted.find((cmd) => cmd.kind === 'searchFind')
    expect(query).toBeDefined()
    expect(query!.text).toBe('needle')
    expect(query!.arg).toBe(SEARCH_FIND_FLAG_CASE_SENSITIVE | SEARCH_FIND_FLAG_REGEX)
    channel.resolve(query!.id, JSON.stringify({ count: 7, activeIndex: 7 }))

    expect(await result).toEqual({ count: 7, activeIndex: 7 })
  })

  it('encodes flag-off finds as 0 (no accidental case/regex bits)', () => {
    const { channel, posted } = captureQueries()
    void channel.searchFindAsync('needle', false, false)
    expect(posted.find((cmd) => cmd.kind === 'searchFind')!.arg).toBe(0)
    channel.dispose() // settle the round-trip so no real 5s timer outlives the test
  })

  it('cancels the superseded in-flight find IMMEDIATELY (null, no worker wait)', async () => {
    const { channel, posted } = captureQueries()
    const first = channel.searchFindAsync('a', false, false)
    const second = channel.searchFindAsync('ab', false, false)

    // The first settles null before ANY worker reply — a slow/blocked worker can't
    // make the superseded request block the newer one.
    expect(await first).toBeNull()

    const queries = posted.filter((cmd) => cmd.kind === 'searchFind')
    expect(queries).toHaveLength(2)
    // The superseded query's LATE reply must not leak anywhere; the newest resolves.
    channel.resolve(queries[0].id, JSON.stringify({ count: 1, activeIndex: 1 }))
    channel.resolve(queries[1].id, JSON.stringify({ count: 2, activeIndex: 2 }))
    expect(await second).toEqual({ count: 2, activeIndex: 2 })
  })

  it('resolves null on timeout and dispose (never hangs the pending UI)', async () => {
    vi.useFakeTimers()
    const { channel } = captureQueries()
    const timedOut = channel.searchFindAsync('a', false, false)
    await vi.advanceTimersByTimeAsync(5_000)
    expect(await timedOut).toBeNull()

    const disposed = channel.searchFindAsync('b', false, false)
    channel.dispose()
    expect(await disposed).toBeNull()
    // Post-dispose finds settle immediately.
    expect(await channel.searchFindAsync('c', false, false)).toBeNull()
  })

  it('resolves null on a malformed reply payload instead of throwing', async () => {
    const { channel, posted } = captureQueries()
    const result = channel.searchFindAsync('a', false, false)
    channel.resolve(posted.find((cmd) => cmd.kind === 'searchFind')!.id, 'not json')
    expect(await result).toBeNull()
  })
})
