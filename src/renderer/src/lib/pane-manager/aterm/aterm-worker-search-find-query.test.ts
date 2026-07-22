// P1 worker side: a find rides the id-correlated query channel. The worker terminal
// answers `{count, activeIndex}` (decoding the flag bits), and the pane dispatch skips
// a queued find once a NEWER one has arrived (superseded queries never block newer).

import { describe, expect, it, vi } from 'vitest'
import { createWorkerTerminal, type WorkerTerminal } from './aterm-worker-terminal'
import { dispatchPaneCommand, type PaneRuntime } from './aterm-worker-pane-dispatch'
import {
  SEARCH_FIND_FLAG_CASE_SENSITIVE,
  SEARCH_FIND_FLAG_REGEX
} from './aterm-worker-search'
import type { EngineHandle } from './aterm-worker-engine-build'
import type { WorkerFrameScheduler } from './aterm-worker-frame-scheduler'

function makeSearchHandle(): { handle: EngineHandle; search: ReturnType<typeof vi.fn> } {
  const engine = {
    display_offset: 0,
    search_display_origin: 0,
    cell_width: 8,
    cell_height: 16,
    scroll_search_line_into_view: () => undefined
  }
  const search = vi.fn(() => new Uint32Array([3, 0, 4, 9, 2, 4]))
  const handle = {
    kind: 'cpu',
    engine: engine as unknown as EngineHandle['engine'],
    memory: { buffer: new ArrayBuffer(0) } as unknown as WebAssembly.Memory,
    process: () => undefined,
    render: () => undefined,
    framebuffer: () => ({ width: 0, height: 0 }),
    search,
    dispose: () => undefined
  } as EngineHandle
  return { handle, search }
}

describe('worker terminal searchFind query', () => {
  it('runs the find, decodes the flag bits, and answers the post-find state as JSON', () => {
    const { handle, search } = makeSearchHandle()
    const term = createWorkerTerminal(handle)

    const value = term.query(
      'searchFind',
      SEARCH_FIND_FLAG_CASE_SENSITIVE | SEARCH_FIND_FLAG_REGEX,
      undefined,
      'needle'
    )

    expect(search).toHaveBeenCalledWith('needle', true, true)
    // Two matches; find selects the LAST (closest to the live bottom), 1-based.
    expect(JSON.parse(value as string)).toEqual({ count: 2, activeIndex: 2 })
  })

  it('treats absent flags/text as a plain case-insensitive literal find', () => {
    const { handle, search } = makeSearchHandle()
    const term = createWorkerTerminal(handle)
    term.query('searchFind', undefined, undefined, 'abc')
    expect(search).toHaveBeenCalledWith('abc', false, false)
  })
})

function makePane(term: WorkerTerminal | null): {
  pane: PaneRuntime
  posted: { id: number; value: unknown }[]
  schedule: ReturnType<typeof vi.fn>
} {
  const posted: { id: number; value: unknown }[] = []
  const schedule = vi.fn()
  const pane = {
    paneId: 1,
    term,
    engineSetters: null,
    engine: null,
    engineKind: null,
    engineMemory: null,
    storedInit: null,
    canvas: null,
    fellBackToCpu: false,
    disposed: false,
    latestSearchFindQueryId: 0,
    chrome: { pad: 0, head: 0 },
    frameScheduler: { schedule } as unknown as WorkerFrameScheduler,
    serializeCache: { schedule: () => undefined, dispose: () => undefined },
    post: (event: unknown) => {
      const e = event as { type: string; id: number; value: unknown }
      if (e.type === 'queryResult') {
        posted.push({ id: e.id, value: e.value })
      }
    }
  } as PaneRuntime
  return { pane, posted, schedule }
}

describe('pane dispatch searchFind supersede skip', () => {
  it('skips a find whose id is older than the newest ARRIVED find (no engine work)', () => {
    const query = vi.fn(() => '{"count":1,"activeIndex":1}')
    const term = { query } as unknown as WorkerTerminal
    const { pane, posted, schedule } = makePane(term)

    // Both finds arrived (entry records the newest id) before the FIRST executes —
    // the flood-backlog case where the old find sat queued behind bulk process.
    pane.latestSearchFindQueryId = 2
    dispatchPaneCommand(pane, { type: 'query', id: 1, kind: 'searchFind', arg: 0, text: 'a' })

    expect(query).not.toHaveBeenCalled() // superseded → the engine search never runs
    expect(schedule).not.toHaveBeenCalled()
    expect(posted).toEqual([{ id: 1, value: null }])

    // The newest find still executes, repaints, and answers.
    dispatchPaneCommand(pane, { type: 'query', id: 2, kind: 'searchFind', arg: 0, text: 'ab' })
    expect(query).toHaveBeenCalledWith('searchFind', 0, undefined, 'ab')
    expect(schedule).toHaveBeenCalledTimes(1)
    expect(posted[1]).toEqual({ id: 2, value: '{"count":1,"activeIndex":1}' })
  })

  it('answers null (not a crash) when the engine is still building', () => {
    const { pane, posted } = makePane(null)
    pane.latestSearchFindQueryId = 1
    dispatchPaneCommand(pane, { type: 'query', id: 1, kind: 'searchFind', arg: 0, text: 'a' })
    expect(posted).toEqual([{ id: 1, value: null }])
  })
})
