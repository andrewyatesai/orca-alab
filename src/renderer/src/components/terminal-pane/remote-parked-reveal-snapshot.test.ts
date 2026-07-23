/**
 * Parked `remote:` pane reveal at the transport seam (ssh-pane-parking.md §3.3).
 *
 * Reveal is the ordinary mount path: the host's initial subscribe snapshot
 * replays with attention suppression. Version skew (old host without the
 * subscribe budget): an oversized initial snapshot is dropped at the 2 MiB
 * replay limit and the transport surfaces onSnapshotOverflow so the pane can
 * restore through the server-bounded requested-snapshot path, whose reply is
 * split via splitRemoteAltScreenSnapshot (#6106).
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { PtyTransport } from './pty-transport-types'
import {
  TerminalStreamOpcode,
  decodeTerminalStreamFrame,
  decodeTerminalStreamJson,
  encodeTerminalStreamFrame,
  encodeTerminalStreamJson,
  encodeTerminalStreamText
} from '../../../../shared/terminal-stream-protocol'

// #6106 fixture shape: pre-TUI shell history + the alternate-screen frame in one blob.
const PRE_TUI_SCROLLBACK = 'user@remote-host:~/repo$ codex\r\nStarting Codex...\r\n'
const ALT_FRAME = '\x1b[?1049h\x1b[2J\x1b[H\x1b[1;1H┌ Codex ┐'

describe('remote parked pane reveal snapshot', () => {
  const runtimeCall = vi.fn()
  const runtimeSubscribe = vi.fn()
  const subscriptionSendBinary = vi.fn()
  let subscriptionCallbacks: {
    onResponse: (response: unknown) => void
    onBinary?: (bytes: Uint8Array<ArrayBufferLike>) => void
    onError?: (error: { code: string; message: string }) => void
    onClose?: () => void
  } | null = null

  beforeEach(() => {
    vi.resetModules()
    vi.clearAllMocks()
    subscriptionCallbacks = null
    runtimeCall.mockResolvedValue({ ok: true, result: { terminal: { handle: 'terminal-1' } } })
    runtimeSubscribe.mockImplementation(
      async (_args: unknown, callbacks: typeof subscriptionCallbacks) => {
        subscriptionCallbacks = callbacks
        queueMicrotask(() =>
          subscriptionCallbacks?.onResponse({ ok: true, result: { type: 'ready' } })
        )
        return { unsubscribe: vi.fn(), sendBinary: subscriptionSendBinary }
      }
    )
    vi.stubGlobal('window', {
      api: {
        runtimeEnvironments: {
          call: runtimeCall,
          subscribe: runtimeSubscribe
        }
      }
    })
  })

  function subscribedStreamId(): number {
    const frame = subscriptionSendBinary.mock.calls
      .map((call) => decodeTerminalStreamFrame(call[0]))
      .findLast((decoded) => decoded?.opcode === TerminalStreamOpcode.Subscribe)
    const payload = frame && decodeTerminalStreamJson<{ streamId: number }>(frame.payload)
    if (!payload) {
      throw new Error('missing terminal subscribe frame')
    }
    return payload.streamId
  }

  function emitSnapshotFrames(
    streamId: number,
    info: Record<string, unknown>,
    chunks: string[]
  ): void {
    subscriptionCallbacks?.onBinary?.(
      encodeTerminalStreamFrame({
        opcode: TerminalStreamOpcode.SnapshotStart,
        streamId,
        seq: 1,
        payload: encodeTerminalStreamJson(info)
      })
    )
    for (const chunk of chunks) {
      subscriptionCallbacks?.onBinary?.(
        encodeTerminalStreamFrame({
          opcode: TerminalStreamOpcode.SnapshotChunk,
          streamId,
          seq: 2,
          payload: encodeTerminalStreamText(chunk)
        })
      )
    }
    subscriptionCallbacks?.onBinary?.(
      encodeTerminalStreamFrame({
        opcode: TerminalStreamOpcode.SnapshotEnd,
        streamId,
        seq: 3,
        payload: new Uint8Array()
      })
    )
  }

  async function attachRevealedPane(callbacks: Record<string, unknown>): Promise<{
    transport: PtyTransport
    streamId: number
  }> {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1'
    })
    transport.attach({
      existingPtyId: 'remote:terminal-1',
      cols: 120,
      rows: 40,
      callbacks
    })
    await vi.waitFor(() => {
      expect(subscriptionSendBinary).toHaveBeenCalled()
      subscribedStreamId()
    })
    return { transport, streamId: subscribedStreamId() }
  }

  it('replays the subscribe snapshot through the replay path with attention suppressed', async () => {
    const onReplayData = vi.fn()
    const onData = vi.fn()
    const onSnapshotOverflow = vi.fn()
    const { streamId } = await attachRevealedPane({ onReplayData, onData, onSnapshotOverflow })

    // A BEL inside the historical snapshot must not re-fire attention on reveal.
    emitSnapshotFrames(streamId, { kind: 'scrollback' }, [`${PRE_TUI_SCROLLBACK}done\x07`])

    expect(onReplayData).toHaveBeenCalledTimes(1)
    expect(onReplayData.mock.calls[0][0]).toBe(`${PRE_TUI_SCROLLBACK}done\x07`)
    expect(onData).not.toHaveBeenCalled()
    expect(onSnapshotOverflow).not.toHaveBeenCalled()
  })

  it('surfaces an oversized initial snapshot as onSnapshotOverflow, not an error, and keeps live output', async () => {
    const onReplayData = vi.fn()
    const onData = vi.fn()
    const onError = vi.fn()
    const onSnapshotOverflow = vi.fn()
    const { streamId } = await attachRevealedPane({
      onReplayData,
      onData,
      onError,
      onSnapshotOverflow
    })

    // Old host without the subscribe budget: > 2 MiB trips the client replay limit.
    emitSnapshotFrames(streamId, { kind: 'scrollback' }, ['x'.repeat(2 * 1024 * 1024 + 1)])

    expect(onSnapshotOverflow).toHaveBeenCalledTimes(1)
    expect(onError).not.toHaveBeenCalled()
    expect(onReplayData).not.toHaveBeenCalled()

    subscriptionCallbacks?.onBinary?.(
      encodeTerminalStreamFrame({
        opcode: TerminalStreamOpcode.Output,
        streamId,
        seq: 4,
        payload: encodeTerminalStreamText('live output')
      })
    )
    expect(onData).toHaveBeenCalledWith('live output', expect.anything())
  })

  it('serves the budgeted requested-snapshot fallback split via splitRemoteAltScreenSnapshot', async () => {
    const onSnapshotOverflow = vi.fn()
    const { transport, streamId } = await attachRevealedPane({ onSnapshotOverflow })

    emitSnapshotFrames(streamId, { kind: 'scrollback' }, ['x'.repeat(2 * 1024 * 1024 + 1)])
    expect(onSnapshotOverflow).toHaveBeenCalledTimes(1)

    // The fallback restore requests a bounded snapshot through the requested path.
    const snapshotPromise = transport.serializeBuffer!({ scrollbackRows: 5_000 })
    await vi.waitFor(() => {
      const requestFrame = subscriptionSendBinary.mock.calls
        .map((call) => decodeTerminalStreamFrame(call[0]))
        .findLast((decoded) => decoded?.opcode === TerminalStreamOpcode.SnapshotRequest)
      expect(requestFrame).toBeTruthy()
    })
    const requestFrame = subscriptionSendBinary.mock.calls
      .map((call) => decodeTerminalStreamFrame(call[0]))
      .findLast((decoded) => decoded?.opcode === TerminalStreamOpcode.SnapshotRequest)!
    const request = decodeTerminalStreamJson<{ requestId: number; scrollbackRows: number }>(
      requestFrame.payload
    )!
    expect(request.scrollbackRows).toBe(5_000)

    // Host reply: one combined blob with the #6106 alt-screen markers.
    emitSnapshotFrames(
      streamId,
      {
        requestId: request.requestId,
        cols: 120,
        rows: 40,
        alternateScreen: true,
        scrollbackChars: PRE_TUI_SCROLLBACK.length
      },
      [PRE_TUI_SCROLLBACK + ALT_FRAME]
    )

    // Split restorer shape: history beside the alt frame, so the alt-screen
    // restore cannot paint pre-TUI history into the alt buffer (#6106).
    await expect(snapshotPromise).resolves.toMatchObject({
      alternateScreen: true,
      scrollbackAnsi: PRE_TUI_SCROLLBACK,
      data: ALT_FRAME
    })
  })
})
