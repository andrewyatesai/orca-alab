import { describe, expect, it, vi } from 'vitest'
import { RpcDispatcher } from './dispatcher'
import type { RpcRequest } from './core'
import type { OrcaRuntimeService } from '../orca-runtime'
import { TERMINAL_METHODS } from './methods/terminal'
import type { RuntimeTerminalWait } from '../../../shared/runtime-types'
import {
  TerminalStreamOpcode,
  decodeTerminalStreamFrame,
  decodeTerminalStreamJson,
  decodeTerminalStreamText,
  encodeTerminalStreamFrame,
  encodeTerminalStreamJson
} from '../../../shared/terminal-stream-protocol'

// Wire regression for #6106: an SSH (remote-runtime) Codex tab restore fetches
// a requested snapshot whose data is history + alt frame in ONE blob. Without
// alternateScreen + scrollbackChars on the SnapshotStart frame the renderer
// cannot split them, replays the pre-TUI history into the alt buffer, and the
// restore's clear wipes it. Repro shape: shell output (pre-TUI scrollback),
// then Codex active on the alternate screen, then a hidden-tab restore request.

// Pre-TUI shell history the daemon keeps in the NORMAL buffer while Codex is on
// the alt screen.
const PRE_TUI_SCROLLBACK = 'user@ssh-host:~/repo$ codex\r\nStarting Codex...\r\n'
// The visible alternate-screen frame (mode rehydrate + TUI paint).
const ALT_FRAME = '\x1b[?1049h\x1b[2J\x1b[H\x1b[1;1H┌ Codex ┐'

function stubRuntime(overrides: Partial<OrcaRuntimeService> = {}): OrcaRuntimeService {
  return {
    getRuntimeId: () => 'test-runtime',
    ...overrides
  } as OrcaRuntimeService
}

function makeRequest(method: string, params?: unknown): RpcRequest {
  return { id: 'req-1', authToken: 'tok', method, params }
}

describe('terminal.multiplex alt-screen snapshot shape (#6106)', () => {
  it('carries alternateScreen + scrollbackChars so a restore can split pre-TUI history from the alt frame', async () => {
    vi.useFakeTimers()
    try {
      const messages: string[] = []
      const binaryFrames: Uint8Array<ArrayBufferLike>[] = []
      const handlers = new Map<
        number,
        (frame: NonNullable<ReturnType<typeof decodeTerminalStreamFrame>>) => void
      >()
      const cleanups = new Map<string, () => void>()
      const runtime = stubRuntime({
        resolveLiveLeafForHandle: vi.fn().mockReturnValue({ ptyId: 'pty-1' }),
        readTerminal: vi.fn().mockResolvedValue({ tail: [], truncated: false }),
        serializeTerminalBuffer: vi.fn().mockResolvedValue({
          data: ALT_FRAME,
          scrollbackAnsi: PRE_TUI_SCROLLBACK,
          alternateScreen: true,
          cols: 80,
          rows: 24,
          seq: 42
        }),
        getTerminalSize: vi.fn().mockReturnValue({ cols: 80, rows: 24 }),
        getMobileDisplayMode: vi.fn().mockReturnValue('auto'),
        getLayout: vi.fn().mockReturnValue({ seq: 1 }),
        registerRemoteTerminalViewSubscriber: vi.fn(() => () => {}),
        subscribeToTerminalData: vi.fn().mockReturnValue(vi.fn()),
        subscribeToTerminalResize: vi.fn().mockReturnValue(vi.fn()),
        subscribeToFitOverrideChanges: vi.fn().mockReturnValue(vi.fn()),
        subscribeToDriverChanges: vi.fn().mockReturnValue(vi.fn()),
        getTerminalFitOverride: vi.fn().mockReturnValue(null),
        getDriver: vi.fn().mockReturnValue({ kind: 'idle' }),
        registerSubscriptionCleanup: vi.fn((id: string, cleanup: () => void) => {
          cleanups.set(id, cleanup)
        }),
        cleanupSubscription: vi.fn((id: string) => {
          const cleanup = cleanups.get(id)
          cleanups.delete(id)
          cleanup?.()
        }),
        waitForTerminal: vi.fn(() => new Promise<RuntimeTerminalWait>(() => {})),
        updateDesktopViewport: vi.fn().mockResolvedValue(true)
      })
      const dispatcher = new RpcDispatcher({
        runtime,
        methods: TERMINAL_METHODS
      })

      const dispatchPromise = dispatcher.dispatchStreaming(
        makeRequest('terminal.multiplex', {}),
        (msg) => messages.push(msg),
        {
          connectionId: 'conn-1',
          sendBinary: (bytes) => {
            binaryFrames.push(bytes)
          },
          registerBinaryStreamHandler: (streamId, handler) => {
            handlers.set(streamId, handler)
            return () => handlers.delete(streamId)
          }
        }
      )

      await vi.runOnlyPendingTimersAsync()
      expect(messages.some((msg) => JSON.parse(msg).result?.type === 'ready')).toBe(true)

      handlers.get(0)?.(
        decodeTerminalStreamFrame(
          encodeTerminalStreamFrame({
            opcode: TerminalStreamOpcode.Subscribe,
            streamId: 0,
            seq: 1,
            payload: encodeTerminalStreamJson({
              streamId: 5,
              terminal: 'terminal-1',
              client: { id: 'desktop-1', type: 'desktop' }
            })
          })
        )!
      )
      for (let i = 0; i < 5; i += 1) {
        await vi.runOnlyPendingTimersAsync()
      }

      // The hidden-tab restore: the desktop client requests a fresh snapshot.
      binaryFrames.length = 0
      handlers.get(5)?.(
        decodeTerminalStreamFrame(
          encodeTerminalStreamFrame({
            opcode: TerminalStreamOpcode.SnapshotRequest,
            streamId: 5,
            seq: 2,
            payload: encodeTerminalStreamJson({ requestId: 9, scrollbackRows: 10_000 })
          })
        )!
      )
      for (let i = 0; i < 5; i += 1) {
        await vi.runOnlyPendingTimersAsync()
      }

      const decoded = binaryFrames.map((frame) => decodeTerminalStreamFrame(frame))
      const snapshotStart = decoded.find(
        (frame) => frame?.opcode === TerminalStreamOpcode.SnapshotStart
      )!
      expect(decodeTerminalStreamJson(snapshotStart.payload)).toMatchObject({
        requestId: 9,
        alternateScreen: true,
        scrollbackChars: PRE_TUI_SCROLLBACK.length
      })
      const snapshotData = decoded
        .filter((frame) => frame?.opcode === TerminalStreamOpcode.SnapshotChunk)
        .map((frame) => decodeTerminalStreamText(frame!.payload))
        .join('')
      // scrollbackChars must index the exact history/frame boundary in data.
      expect(snapshotData).toBe(PRE_TUI_SCROLLBACK + ALT_FRAME)
      expect(snapshotData.slice(0, PRE_TUI_SCROLLBACK.length)).toBe(PRE_TUI_SCROLLBACK)

      runtime.cleanupSubscription('terminal-multiplex:conn-1')
      await dispatchPromise
    } finally {
      vi.useRealTimers()
    }
  })
})
