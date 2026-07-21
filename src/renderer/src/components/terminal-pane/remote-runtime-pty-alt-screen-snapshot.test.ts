import { beforeEach, describe, expect, it, vi } from 'vitest'
import {
  TerminalStreamOpcode,
  decodeTerminalStreamFrame,
  decodeTerminalStreamJson,
  encodeTerminalStreamFrame,
  encodeTerminalStreamJson,
  encodeTerminalStreamText
} from '../../../../shared/terminal-stream-protocol'
import { splitRemoteAltScreenSnapshot } from './remote-runtime-pty-alt-screen-snapshot'

// #6106: an SSH terminal running Codex loses its pre-TUI shell scrollback after
// a hidden-tab restore. The remote wire delivers ONE blob (history + alt frame);
// alternateScreen + scrollbackChars let the transport rebuild the local-daemon
// snapshot shape (separate scrollbackAnsi) that the alt-screen restorer needs.

// Pre-TUI shell output the user scrolled past before launching Codex.
const PRE_TUI_SCROLLBACK = 'user@ssh-host:~/repo$ codex\r\nStarting Codex...\r\n'
const ALT_FRAME = '\x1b[?1049h\x1b[2J\x1b[H\x1b[1;1H┌ Codex ┐'

describe('splitRemoteAltScreenSnapshot (#6106)', () => {
  it('splits an alt-screen snapshot into scrollbackAnsi + alt frame at scrollbackChars', () => {
    const split = splitRemoteAltScreenSnapshot({
      data: PRE_TUI_SCROLLBACK + ALT_FRAME,
      cols: 80,
      rows: 24,
      seq: 42,
      source: 'headless',
      alternateScreen: true,
      scrollbackChars: PRE_TUI_SCROLLBACK.length
    })
    expect(split).toMatchObject({
      alternateScreen: true,
      scrollbackAnsi: PRE_TUI_SCROLLBACK,
      data: ALT_FRAME,
      seq: 42
    })
  })

  it('keeps an alt-screen snapshot unsplit when the runtime reports no history', () => {
    const split = splitRemoteAltScreenSnapshot({
      data: ALT_FRAME,
      cols: 80,
      rows: 24,
      alternateScreen: true,
      scrollbackChars: 0
    })
    // No scrollbackAnsi: the restorer preserves the pane's existing scrollback.
    expect(split.scrollbackAnsi).toBeUndefined()
    expect(split).toMatchObject({ alternateScreen: true, data: ALT_FRAME })
  })

  it('passes a normal-buffer snapshot through untouched', () => {
    const snapshot = {
      data: `${PRE_TUI_SCROLLBACK}user@ssh-host:~/repo$ `,
      cols: 80,
      rows: 24,
      scrollbackChars: PRE_TUI_SCROLLBACK.length
    }
    const split = splitRemoteAltScreenSnapshot(snapshot)
    expect(split.scrollbackAnsi).toBeUndefined()
    expect(split.data).toBe(snapshot.data)
  })

  it('clamps a scrollbackChars boundary that exceeds the delivered data', () => {
    const split = splitRemoteAltScreenSnapshot({
      data: ALT_FRAME,
      cols: 80,
      rows: 24,
      alternateScreen: true,
      scrollbackChars: ALT_FRAME.length + 100
    })
    expect(split.scrollbackAnsi).toBe(ALT_FRAME)
    expect(split.data).toBe('')
  })
})

// Wire half: drives REAL binary frames through the REAL multiplexer and
// transport, mirroring remote-runtime-pty-snapshot-escape-tail.test.ts, so the
// requested-snapshot lane cannot silently drop the alt-screen shape.
describe('remote transport requested-snapshot alt-screen threading (#6106)', () => {
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
    vi.doUnmock('../../runtime/remote-runtime-terminal-multiplexer')
    vi.clearAllMocks()
    subscriptionCallbacks = null
    subscriptionSendBinary.mockReset()
    runtimeCall.mockResolvedValue({ ok: true, result: { terminal: { handle: 'terminal-1' } } })
    runtimeSubscribe.mockImplementation(
      async (_args: unknown, callbacks: typeof subscriptionCallbacks) => {
        subscriptionCallbacks = callbacks
        return { unsubscribe: vi.fn(), sendBinary: subscriptionSendBinary }
      }
    )
    vi.stubGlobal('window', {
      api: {
        runtimeEnvironments: { call: runtimeCall, subscribe: runtimeSubscribe }
      }
    })
  })

  it('resolves serializeBuffer with the split alt-screen restore shape', async () => {
    const { createRemoteRuntimePtyTransport } = await import('./remote-runtime-pty-transport')
    const transport = createRemoteRuntimePtyTransport('env-1', {
      worktreeId: 'wt-1',
      tabId: 'tab-1',
      leafId: 'pane:1'
    })
    transport.attach({
      existingPtyId: 'remote:env-1@@terminal-1',
      cols: 80,
      rows: 24,
      callbacks: {}
    })

    await expect.poll(() => subscriptionCallbacks !== null, { timeout: 5000 }).toBe(true)
    subscriptionCallbacks?.onResponse({ ok: true, result: { type: 'ready' } })

    await expect
      .poll(() => subscriptionSendBinary.mock.calls.length, { timeout: 5000 })
      .toBeGreaterThan(0)
    const subscribeFrame = subscriptionSendBinary.mock.calls
      .map((call) => decodeTerminalStreamFrame(call[0] as Uint8Array))
      .find((frame) => frame?.opcode === TerminalStreamOpcode.Subscribe)
    expect(subscribeFrame).toBeDefined()
    const subscribePayload = decodeTerminalStreamJson<{ streamId: number }>(subscribeFrame!.payload)
    const streamId = subscribePayload!.streamId

    // The hidden-tab restore issues a requested snapshot over the live stream.
    const framesSentBeforeRequest = subscriptionSendBinary.mock.calls.length
    const snapshotPromise = transport.serializeBuffer!({ scrollbackRows: 10_000 })
    await expect
      .poll(() => subscriptionSendBinary.mock.calls.length, { timeout: 5000 })
      .toBeGreaterThan(framesSentBeforeRequest)
    const requestFrame = subscriptionSendBinary.mock.calls
      .slice(framesSentBeforeRequest)
      .map((call) => decodeTerminalStreamFrame(call[0] as Uint8Array))
      .find((frame) => frame?.opcode === TerminalStreamOpcode.SnapshotRequest)
    expect(requestFrame).toBeDefined()
    const requestId = decodeTerminalStreamJson<{ requestId: number }>(
      requestFrame!.payload
    )!.requestId

    // Server → client: Codex sits on the alt screen; the pre-TUI shell history
    // rides the same blob with scrollbackChars marking the boundary.
    const frames = [
      encodeTerminalStreamFrame({
        opcode: TerminalStreamOpcode.SnapshotStart,
        streamId,
        seq: 0,
        payload: encodeTerminalStreamJson({
          cols: 80,
          rows: 24,
          seq: 42,
          source: 'headless',
          requestId,
          alternateScreen: true,
          scrollbackChars: PRE_TUI_SCROLLBACK.length
        })
      }),
      encodeTerminalStreamFrame({
        opcode: TerminalStreamOpcode.SnapshotChunk,
        streamId,
        seq: 0,
        payload: encodeTerminalStreamText(PRE_TUI_SCROLLBACK + ALT_FRAME)
      }),
      encodeTerminalStreamFrame({
        opcode: TerminalStreamOpcode.SnapshotEnd,
        streamId,
        seq: 0,
        payload: new Uint8Array(0)
      })
    ]
    for (const frame of frames) {
      subscriptionCallbacks?.onBinary?.(frame)
    }

    await expect(snapshotPromise).resolves.toMatchObject({
      alternateScreen: true,
      scrollbackAnsi: PRE_TUI_SCROLLBACK,
      data: ALT_FRAME,
      cols: 80,
      rows: 24,
      seq: 42
    })
  })
})
