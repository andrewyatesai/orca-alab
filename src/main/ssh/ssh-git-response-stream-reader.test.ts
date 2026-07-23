import { describe, expect, it, vi } from 'vitest'
import type { SshChannelMultiplexer } from './ssh-channel-multiplexer'
import { requestGitStreamable } from './ssh-git-response-stream-reader'
import { GIT_RESPONSE_CHUNK_SIZE } from './relay-protocol'

type MethodHandler = (params: Record<string, unknown>) => void

function createMockMux(marker: { streamId: number; totalBytes: number; chunkCount: number }): {
  mux: SshChannelMultiplexer
  emitChunk: (params: Record<string, unknown>) => void
} {
  const handlers = new Map<string, Set<MethodHandler>>()
  const emit = (method: string, params: Record<string, unknown>): void => {
    for (const handler of handlers.get(method) ?? []) {
      handler(params)
    }
  }
  const mux = {
    request: vi.fn().mockResolvedValue({ __orcaGitResponseStream: marker }),
    notify: vi.fn(),
    onNotificationByMethod: vi.fn((method: string, handler: MethodHandler) => {
      let set = handlers.get(method)
      if (!set) {
        set = new Set()
        handlers.set(method, set)
      }
      set.add(handler)
      return () => set!.delete(handler)
    }),
    onDispose: vi.fn().mockReturnValue(() => {}),
    isDisposed: vi.fn().mockReturnValue(false)
  } as unknown as SshChannelMultiplexer
  return {
    mux,
    emitChunk: (params: Record<string, unknown>) => emit('git.responseChunk', params)
  }
}

const chunkData = Buffer.alloc(GIT_RESPONSE_CHUNK_SIZE, 0x61).toString('base64')

describe('requestGitStreamable memory bounds', () => {
  it('rejects a stream that exceeds its declared chunk count instead of buffering forever', async () => {
    // Relay declares a tiny stream but then floods in-order chunks without ever
    // sending responseEnd — the OOM scenario. It must fail, not grow unbounded.
    const { mux, emitChunk } = createMockMux({
      streamId: 7,
      totalBytes: GIT_RESPONSE_CHUNK_SIZE,
      chunkCount: 1
    })
    const promise = requestGitStreamable(mux, 'git.diff', {})
    // Let the sentinel resolve so metadata is applied.
    await Promise.resolve()
    await Promise.resolve()

    // seq 0 is within the declared single chunk; seq 1 overruns chunkCount.
    emitChunk({ streamId: 7, seq: 0, data: chunkData })
    emitChunk({ streamId: 7, seq: 1, data: chunkData })

    await expect(promise).rejects.toThrow(/exceeded declared chunk count/)
  })

  it('rejects a chunk that overruns the declared byte count', async () => {
    const { mux, emitChunk } = createMockMux({
      streamId: 9,
      totalBytes: 4,
      chunkCount: 4
    })
    const promise = requestGitStreamable(mux, 'git.diff', {})
    await Promise.resolve()
    await Promise.resolve()

    // A single chunk larger than totalBytes must be refused up front.
    emitChunk({ streamId: 9, seq: 0, data: Buffer.from('too-long').toString('base64') })

    await expect(promise).rejects.toThrow(/exceeded declared byte count/)
  })

  it('rejects a marker whose declared totals exceed the client cap before reassembly', async () => {
    const { mux } = createMockMux({
      streamId: 11,
      totalBytes: 1024 * 1024 * 1024,
      chunkCount: 1
    })
    const promise = requestGitStreamable(mux, 'git.diff', {})

    await expect(promise).rejects.toThrow(/too large/)
    // The reader never armed the inactivity timer / subscribed to chunks for it.
    expect(mux.notify).not.toHaveBeenCalled()
  })
})
