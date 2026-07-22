import { describe, expect, it } from 'vitest'
import {
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

async function settle(): Promise<void> {
  // Chunk fetches await once per chunk; a few microtask turns settle the whole task.
  for (let i = 0; i < 10; i++) {
    await Promise.resolve()
  }
}

describe('startTerminalScrollbackDeepRestore (P5)', () => {
  it('rebuilds the pane as clear + older chunks + tail + mode reset, in one atomic batch', async () => {
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
      readOlderChunk: reader
    })
    await settle()

    // Sequential cursor stream: each request continues where the last chunk ended.
    expect(calls.map((c) => c.cursor)).toEqual([0, 6])
    expect(writes).toEqual([CLEAR, 'OLD-1|OLD-2|TAIL', '\r\n', POST_REPLAY_MODE_RESET])
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
      readOlderChunk: reader
    })
    await settle()

    expect(writes).toEqual([])
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
      readOlderChunk: reader
    })
    cancel()
    resolveChunk({ text: 'OLD', nextCursor: 10 })
    await settle()

    expect(writes).toEqual([])
  })

  it('aborts when the snapshot changed on disk (reader returns null)', async () => {
    const { pane, writes } = createFakePane()
    const { reader } = chunkedReader([]) // immediately null
    startTerminalScrollbackDeepRestore({
      pane,
      manager: fakeManager,
      source: makeSource(),
      replayingPanesRef: makeReplayingRef(),
      readOlderChunk: reader
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
      readOlderChunk: reader
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
      readOlderChunk: reader
    })
    await settle()

    expect(writes).toEqual([CLEAR, 'before', '\r\n', POST_REPLAY_MODE_RESET])
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
      readOlderChunk: reader
    })
    await settle()

    expect(writes).toEqual([])
  })
})
