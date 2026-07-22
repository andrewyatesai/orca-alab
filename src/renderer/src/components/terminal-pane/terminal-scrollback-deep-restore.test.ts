import { describe, expect, it } from 'vitest'
import {
  altScreenReplayEndOffset,
  DEEP_RESTORE_REPLAY_SLICE_CHARS,
  startTerminalScrollbackDeepRestore,
  type TerminalScrollbackDeepRestoreSource,
  type TerminalScrollbackOlderChunkReader
} from './terminal-scrollback-deep-restore'
import { POST_REPLAY_MODE_RESET } from './layout-serialization'
import { bumpTerminalWriteGeneration } from '@/lib/pane-manager/terminal-write-generation'
import type { ManagedPane, PaneManager } from '@/lib/pane-manager/pane-manager'
import type { ReplayingPanesRef } from './replay-guard'

const CLEAR = '\x1b[2J\x1b[3J\x1b[H'

function createFakePane(): { pane: ManagedPane; writes: string[] } {
  const writes: string[] = []
  const terminal = {
    // Why synchronous callback: the replay guard releases on write completion; tests need no timers.
    write: (data: string, callback?: () => void) => {
      writes.push(data)
      callback?.()
    }
  }
  return { pane: { id: 1, terminal } as unknown as ManagedPane, writes }
}

const fakeManager = { hasWebglRenderer: () => false } as unknown as PaneManager

function makeReplayingRef(): ReplayingPanesRef {
  return { current: new Map() }
}

function makeSource(
  overrides: Partial<TerminalScrollbackDeepRestoreSource> = {}
): TerminalScrollbackDeepRestoreSource {
  return {
    ref: 'v1-ref',
    tailText: 'TAIL',
    olderChunkCursor: 0,
    olderEndOffset: 10,
    fingerprint: 'fp',
    ...overrides
  }
}

function chunkedReader(chunks: { text: string; nextCursor: number }[]): {
  reader: TerminalScrollbackOlderChunkReader
  calls: { cursor: number }[]
} {
  const calls: { cursor: number }[] = []
  let index = 0
  return {
    calls,
    reader: async ({ cursor }) => {
      calls.push({ cursor })
      return chunks[index++] ?? null
    }
  }
}

// Microtask yield so tests drive slice turns without real macrotask timers.
const microtaskYield = (): Promise<void> => Promise.resolve()

/** Resolver-gated yield so tests can act (cancel / live write) BETWEEN slices. */
function steppedYield(): { yield: () => Promise<void>; step: () => void; pending: () => number } {
  const waiters: (() => void)[] = []
  return {
    yield: () =>
      new Promise<void>((resolve) => {
        waiters.push(resolve)
      }),
    step: () => waiters.shift()?.(),
    pending: () => waiters.length
  }
}

async function settle(): Promise<void> {
  // Chunk fetches and slice turns each await once; enough turns settle the task.
  for (let i = 0; i < 100; i++) {
    await Promise.resolve()
  }
}

describe('startTerminalScrollbackDeepRestore (P5)', () => {
  it('rebuilds the pane as clear + older chunks + tail + mode reset, oldest to newest', async () => {
    const { pane, writes } = createFakePane()
    const { reader, calls } = chunkedReader([
      { text: 'OLD-1|', nextCursor: 6 },
      { text: 'OLD-2|', nextCursor: 10 }
    ])
    startTerminalScrollbackDeepRestore({
      pane,
      manager: fakeManager,
      source: makeSource(),
      replayingPanesRef: makeReplayingRef(),
      readOlderChunk: reader,
      yieldBetweenSlices: microtaskYield
    })
    await settle()

    // Sequential cursor stream: each request continues where the last chunk ended.
    expect(calls.map((c) => c.cursor)).toEqual([0, 6])
    // Each chunk feeds in its own slice turn; concatenation order is preserved.
    expect(writes).toEqual([CLEAR, 'OLD-1|', 'OLD-2|', 'TAIL', '\r\n', POST_REPLAY_MODE_RESET])
  })

  it('bounds every engine feed to the slice limit and preserves byte order', async () => {
    const { pane, writes } = createFakePane()
    // One over-limit chunk must split; the split point must not reorder bytes.
    const older = `${'a'.repeat(DEEP_RESTORE_REPLAY_SLICE_CHARS)}${'b'.repeat(1024)}`
    const { reader } = chunkedReader([{ text: older, nextCursor: 10 }])
    startTerminalScrollbackDeepRestore({
      pane,
      manager: fakeManager,
      source: makeSource(),
      replayingPanesRef: makeReplayingRef(),
      readOlderChunk: reader,
      yieldBetweenSlices: microtaskYield
    })
    await settle()

    for (const write of writes) {
      expect(write.length).toBeLessThanOrEqual(DEEP_RESTORE_REPLAY_SLICE_CHARS)
    }
    // More than one content slice actually happened (the bound is real)...
    expect(writes.length).toBeGreaterThan(4)
    // ...and reassembling the writes yields the exact original stream.
    expect(writes.join('')).toBe(`${CLEAR}${older}TAIL\r\n${POST_REPLAY_MODE_RESET}`)
  })

  it('aborts without writing when another writer touched the terminal mid-stream', async () => {
    const { pane, writes } = createFakePane()
    const reader: TerminalScrollbackOlderChunkReader = async ({ cursor }) => {
      // Live PTY output lands while the first chunk is in flight.
      bumpTerminalWriteGeneration(pane.terminal)
      return { text: 'OLD', nextCursor: cursor + 10 }
    }
    startTerminalScrollbackDeepRestore({
      pane,
      manager: fakeManager,
      source: makeSource(),
      replayingPanesRef: makeReplayingRef(),
      readOlderChunk: reader,
      yieldBetweenSlices: microtaskYield
    })
    await settle()

    expect(writes).toEqual([])
  })

  it('stops mid-replay (keeping order) and resets modes when live output lands between slices', async () => {
    const { pane, writes } = createFakePane()
    const stepped = steppedYield()
    const { reader } = chunkedReader([
      { text: 'OLD-1|', nextCursor: 6 },
      { text: 'OLD-2|', nextCursor: 10 }
    ])
    startTerminalScrollbackDeepRestore({
      pane,
      manager: fakeManager,
      source: makeSource(),
      replayingPanesRef: makeReplayingRef(),
      readOlderChunk: reader,
      yieldBetweenSlices: stepped.yield
    })
    await settle()
    // First turn painted clear + oldest slice, then parked on the yield.
    expect(writes).toEqual([CLEAR, 'OLD-1|'])

    // Live PTY bytes enqueue between slices (both funnels bump at enqueue).
    bumpTerminalWriteGeneration(pane.terminal)
    stepped.step()
    await settle()

    // No further history slices (no interleaved rebuild), one mode reset only.
    expect(writes).toEqual([CLEAR, 'OLD-1|', POST_REPLAY_MODE_RESET])
    expect(stepped.pending()).toBe(0)
  })

  it('aborts when cancelled (pane unmount) before chunks finish', async () => {
    const { pane, writes } = createFakePane()
    let resolveChunk!: (chunk: { text: string; nextCursor: number }) => void
    const reader: TerminalScrollbackOlderChunkReader = () =>
      new Promise((resolve) => {
        resolveChunk = resolve
      })
    const cancel = startTerminalScrollbackDeepRestore({
      pane,
      manager: fakeManager,
      source: makeSource(),
      replayingPanesRef: makeReplayingRef(),
      readOlderChunk: reader,
      yieldBetweenSlices: microtaskYield
    })
    cancel()
    resolveChunk({ text: 'OLD', nextCursor: 10 })
    await settle()

    expect(writes).toEqual([])
  })

  it('aborts cleanly when the pane closes mid-hydration (no further writes)', async () => {
    const { pane, writes } = createFakePane()
    const stepped = steppedYield()
    const { reader } = chunkedReader([
      { text: 'OLD-1|', nextCursor: 6 },
      { text: 'OLD-2|', nextCursor: 10 }
    ])
    const cancel = startTerminalScrollbackDeepRestore({
      pane,
      manager: fakeManager,
      source: makeSource(),
      replayingPanesRef: makeReplayingRef(),
      readOlderChunk: reader,
      yieldBetweenSlices: stepped.yield
    })
    await settle()
    expect(writes).toEqual([CLEAR, 'OLD-1|'])

    cancel()
    stepped.step()
    await settle()

    // Silent stop: a closing pane gets no more slices and no epilogue.
    expect(writes).toEqual([CLEAR, 'OLD-1|'])
    expect(stepped.pending()).toBe(0)
  })

  it('aborts when the snapshot changed on disk (reader returns null)', async () => {
    const { pane, writes } = createFakePane()
    const { reader } = chunkedReader([]) // immediately null
    startTerminalScrollbackDeepRestore({
      pane,
      manager: fakeManager,
      source: makeSource(),
      replayingPanesRef: makeReplayingRef(),
      readOlderChunk: reader,
      yieldBetweenSlices: microtaskYield
    })
    await settle()

    expect(writes).toEqual([])
  })

  it('aborts when a chunk makes no forward progress (defends against loops)', async () => {
    const { pane, writes } = createFakePane()
    const { reader, calls } = chunkedReader([{ text: '', nextCursor: 0 }])
    startTerminalScrollbackDeepRestore({
      pane,
      manager: fakeManager,
      source: makeSource(),
      replayingPanesRef: makeReplayingRef(),
      readOlderChunk: reader,
      yieldBetweenSlices: microtaskYield
    })
    await settle()

    expect(calls).toHaveLength(1)
    expect(writes).toEqual([])
  })

  it('trims an unmatched alt-screen enter across the older/tail seam', async () => {
    const { pane, writes } = createFakePane()
    const { reader } = chunkedReader([{ text: 'before\x1b[?1049hTUI-', nextCursor: 10 }])
    startTerminalScrollbackDeepRestore({
      pane,
      manager: fakeManager,
      source: makeSource({ tailText: 'frame' }),
      replayingPanesRef: makeReplayingRef(),
      readOlderChunk: reader,
      yieldBetweenSlices: microtaskYield
    })
    await settle()

    expect(writes).toEqual([CLEAR, 'before', '\r\n', POST_REPLAY_MODE_RESET])
  })

  it('trims an alt-screen enter that straddles a chunk boundary', async () => {
    const { pane, writes } = createFakePane()
    const { reader } = chunkedReader([
      { text: 'AB\x1b[?10', nextCursor: 7 },
      { text: '49hCD', nextCursor: 10 }
    ])
    startTerminalScrollbackDeepRestore({
      pane,
      manager: fakeManager,
      source: makeSource({ tailText: 'EF' }),
      replayingPanesRef: makeReplayingRef(),
      readOlderChunk: reader,
      yieldBetweenSlices: microtaskYield
    })
    await settle()

    expect(writes).toEqual([CLEAR, 'AB', '\r\n', POST_REPLAY_MODE_RESET])
  })

  it('leaves a disposed pane untouched', async () => {
    const { pane, writes } = createFakePane()
    ;(pane.terminal as unknown as { isDisposed: boolean }).isDisposed = true
    const { reader } = chunkedReader([{ text: 'OLD', nextCursor: 10 }])
    startTerminalScrollbackDeepRestore({
      pane,
      manager: fakeManager,
      source: makeSource(),
      replayingPanesRef: makeReplayingRef(),
      readOlderChunk: reader,
      yieldBetweenSlices: microtaskYield
    })
    await settle()

    expect(writes).toEqual([])
  })
})

describe('altScreenReplayEndOffset', () => {
  it('matches trimTrailingAltScreenEnter semantics over the concatenation', () => {
    expect(altScreenReplayEndOffset(['plain', 'text'])).toBe(9)
    expect(altScreenReplayEndOffset(['a\x1b[?1049h', 'tui'])).toBe(1)
    // A later leave rebalances the enter: nothing is trimmed.
    expect(altScreenReplayEndOffset(['a\x1b[?1049h', 'tui\x1b[?1049l', 'after'])).toBe(25)
    // Straddled leave also rebalances (split mid-sequence across pieces).
    expect(altScreenReplayEndOffset(['a\x1b[?1049htui\x1b[?10', '49lafter'])).toBe(25)
  })
})
