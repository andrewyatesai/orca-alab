// Fed §2.4 client side: the multiplexer must record hostRowAnchor/anchorGen
// from the snapshot it actually REPLAYS (initial/recovery), reset it when a
// replayed snapshot carries no anchor, and never record it from a
// requested-but-unreplayed snapshot — the generation gate that stops
// wrong-row jumps.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  TerminalStreamOpcode,
  decodeTerminalStreamFrame,
  decodeTerminalStreamJson,
  encodeTerminalStreamFrame,
  encodeTerminalStreamJson,
  encodeTerminalStreamText
} from '../../../shared/terminal-stream-protocol'
import {
  getRemoteRuntimeTerminalMultiplexer,
  resetRemoteRuntimeTerminalMultiplexersForTests,
  type RemoteRuntimeMultiplexedTerminal,
  type RemoteRuntimeReplayedHostAnchor
} from './remote-runtime-terminal-multiplexer'

type SubscribeCallbacks = {
  onResponse: (response: unknown) => void
  onBinary?: (bytes: Uint8Array<ArrayBufferLike>) => void
  onError?: (error: { message: string }) => void
  onClose?: () => void
}

class FakeAnchorServer {
  private streamId = 0

  constructor(private readonly toClient: (bytes: Uint8Array<ArrayBufferLike>) => void) {}

  receive(bytes: Uint8Array<ArrayBufferLike>): void {
    const frame = decodeTerminalStreamFrame(bytes)
    if (!frame) {
      return
    }
    if (frame.opcode === TerminalStreamOpcode.Subscribe) {
      const payload = decodeTerminalStreamJson<{ streamId: number }>(frame.payload)
      this.streamId = payload?.streamId ?? 0
      this.sendSnapshot('INITIAL', { hostRowAnchor: 98, anchorGen: 41 })
    }
    if (frame.opcode === TerminalStreamOpcode.SnapshotRequest) {
      const payload = decodeTerminalStreamJson<{ requestId?: number }>(frame.payload)
      // A REQUESTED snapshot (park/serialize path) carries an anchor too, but
      // it is not replayed into the engine — the client must not adopt it.
      this.sendSnapshot('REQUESTED', { hostRowAnchor: 205, anchorGen: 77 }, payload?.requestId)
    }
  }

  sendSnapshot(
    data: string,
    anchor: { hostRowAnchor: number; anchorGen: number } | null,
    requestId?: number
  ): void {
    const send = (opcode: TerminalStreamOpcode, payload: Uint8Array): void => {
      this.toClient(encodeTerminalStreamFrame({ opcode, streamId: this.streamId, seq: 0, payload }))
    }
    send(
      TerminalStreamOpcode.SnapshotStart,
      encodeTerminalStreamJson({ cols: 80, rows: 24, seq: 0, requestId, ...anchor })
    )
    send(TerminalStreamOpcode.SnapshotChunk, encodeTerminalStreamText(data))
    send(TerminalStreamOpcode.SnapshotEnd, new Uint8Array())
  }
}

describe('remote terminal snapshot anchor recording', () => {
  const unsubscribe = vi.fn()
  let server: FakeAnchorServer

  beforeEach(() => {
    vi.clearAllMocks()
    resetRemoteRuntimeTerminalMultiplexersForTests()
    const subscribe = vi.fn(async (_args: unknown, callbacks: SubscribeCallbacks) => {
      server = new FakeAnchorServer((bytes) => callbacks.onBinary?.(bytes))
      queueMicrotask(() => callbacks.onResponse({ ok: true, result: { type: 'ready' } }))
      return {
        unsubscribe,
        sendBinary: (bytes: Uint8Array<ArrayBufferLike>) => server.receive(bytes)
      }
    })
    vi.stubGlobal('window', { api: { runtimeEnvironments: { subscribe } } })
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  async function subscribeClient(): Promise<{
    snapshots: string[]
    snapshotAnchors: (RemoteRuntimeReplayedHostAnchor | undefined)[]
    stream: RemoteRuntimeMultiplexedTerminal
  }> {
    const snapshots: string[] = []
    const snapshotAnchors: (RemoteRuntimeReplayedHostAnchor | undefined)[] = []
    const multiplexer = getRemoteRuntimeTerminalMultiplexer('env-1')
    const stream = await multiplexer.subscribeTerminal({
      terminal: 'terminal-1',
      client: { id: 'desktop-1', type: 'desktop' },
      callbacks: {
        onData: () => {},
        onSnapshot: (chunk, meta) => {
          snapshots.push(chunk)
          snapshotAnchors.push(meta?.replayedHostAnchor)
        }
      }
    })
    await Promise.resolve()
    await Promise.resolve()
    return { snapshots, snapshotAnchors, stream }
  }

  it('records the anchor of the replayed initial snapshot and hands it to onSnapshot', async () => {
    const { snapshots, snapshotAnchors, stream } = await subscribeClient()
    expect(snapshots).toEqual(['INITIAL'])
    expect(snapshotAnchors).toEqual([{ hostRowAnchor: 98, anchorGen: 41 }])
    expect(stream.getReplayedHostAnchor()).toEqual({ hostRowAnchor: 98, anchorGen: 41 })
  })

  it('does NOT adopt the anchor of a requested (un-replayed) snapshot', async () => {
    const { stream } = await subscribeClient()
    const requested = await stream.serializeBuffer()
    expect(requested?.data).toBe('REQUESTED')
    // The engine still holds the INITIAL replay — its anchor must survive.
    expect(stream.getReplayedHostAnchor()).toEqual({ hostRowAnchor: 98, anchorGen: 41 })
  })

  it('resets the anchor when a recovery snapshot without one replaces engine state', async () => {
    const { snapshots, stream } = await subscribeClient()
    // Server-pushed recovery (no requestId, after initial): replaces the
    // engine contents; an anchor-less replacement must clear the stale anchor.
    server.sendSnapshot('RECOVERY', null)
    await Promise.resolve()
    expect(snapshots.some((s) => s.includes('RECOVERY'))).toBe(true)
    expect(stream.getReplayedHostAnchor()).toBeNull()
  })

  it('adopts a NEW anchor from an anchored recovery snapshot', async () => {
    const { stream } = await subscribeClient()
    server.sendSnapshot('RECOVERY2', { hostRowAnchor: 300, anchorGen: 55 })
    await Promise.resolve()
    expect(stream.getReplayedHostAnchor()).toEqual({ hostRowAnchor: 300, anchorGen: 55 })
  })
})
