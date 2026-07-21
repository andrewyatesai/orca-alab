/**
 * Regression for #8871: the multiplex exit-waiter used to convert waiter
 * REJECTION (e.g. 'terminal_handle_stale' when the host renderer reloads and
 * re-mints handles) into an exit-shaped 'end' event. Remote mirrors treat 'end'
 * as PTY exit and close the tab host-side, killing live host terminal sessions.
 * A rejection must surface as a stream error (quiet retire) — never 'end'.
 */
import { describe, expect, it, vi } from 'vitest'
import { RpcDispatcher } from './dispatcher'
import type { RpcRequest } from './core'
import type { OrcaRuntimeService } from '../orca-runtime'
import { TERMINAL_METHODS } from './methods/terminal'
import type { RuntimeTerminalWait } from '../../../shared/runtime-types'
import {
  TerminalStreamOpcode,
  decodeTerminalStreamFrame,
  encodeTerminalStreamFrame,
  encodeTerminalStreamJson
} from '../../../shared/terminal-stream-protocol'

type ExitWaiterControl = {
  resolve: (wait: RuntimeTerminalWait) => void
  reject: (error: Error) => void
}

function stubRuntime(exitWaiter: ExitWaiterControl): OrcaRuntimeService {
  const cleanups = new Map<string, () => void>()
  return {
    getRuntimeId: () => 'test-runtime',
    registerRemoteTerminalViewSubscriber: () => () => {},
    resolveLiveLeafForHandle: vi.fn().mockReturnValue({ ptyId: 'pty-1' }),
    updateRemoteDesktopViewer: vi.fn().mockResolvedValue(true),
    unregisterRemoteDesktopViewer: vi.fn().mockResolvedValue(true),
    unregisterRemoteDesktopViewers: vi.fn().mockResolvedValue(true),
    isPtyResizeDrivenRemotely: vi.fn().mockReturnValue(false),
    getRemoteDesktopFitHold: vi.fn().mockReturnValue({ mode: 'desktop-fit', cols: 120, rows: 40 }),
    isRemoteDesktopViewerOwner: vi.fn().mockReturnValue(false),
    readTerminal: vi.fn().mockResolvedValue({ tail: [], truncated: false }),
    serializeTerminalBuffer: vi.fn().mockResolvedValue({ data: 'snapshot', cols: 120, rows: 40 }),
    getTerminalSize: vi.fn().mockReturnValue({ cols: 120, rows: 40 }),
    getMobileDisplayMode: vi.fn().mockReturnValue('auto'),
    getLayout: vi.fn().mockReturnValue({ seq: 1 }),
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
    waitForTerminal: vi.fn(
      () =>
        new Promise<RuntimeTerminalWait>((resolve, reject) => {
          exitWaiter.resolve = resolve
          exitWaiter.reject = reject
        })
    ),
    sendTerminal: vi.fn().mockResolvedValue({ accepted: true })
  } as unknown as OrcaRuntimeService
}

async function subscribeMultiplexStream(runtime: OrcaRuntimeService): Promise<{
  messages: string[]
  eventsForStream: (streamId: number) => { type?: string; message?: string }[]
}> {
  const messages: string[] = []
  const handlers = new Map<
    number,
    (frame: NonNullable<ReturnType<typeof decodeTerminalStreamFrame>>) => void
  >()
  const dispatcher = new RpcDispatcher({ runtime, methods: TERMINAL_METHODS })
  const request: RpcRequest = { id: 'req-1', authToken: 'tok', method: 'terminal.multiplex' }
  void dispatcher.dispatchStreaming(request, (msg) => messages.push(msg), {
    connectionId: 'conn-1',
    sendBinary: vi.fn(),
    registerBinaryStreamHandler: (streamId, handler) => {
      handlers.set(streamId, handler)
      return () => handlers.delete(streamId)
    }
  })
  await vi.waitFor(() =>
    expect(messages.some((msg) => JSON.parse(msg).result?.type === 'ready')).toBe(true)
  )
  handlers.get(0)?.(
    decodeTerminalStreamFrame(
      encodeTerminalStreamFrame({
        opcode: TerminalStreamOpcode.Subscribe,
        streamId: 0,
        seq: 1,
        payload: encodeTerminalStreamJson({
          streamId: 5,
          terminal: 'terminal-1',
          client: { id: 'desktop-1', type: 'desktop' },
          viewport: { cols: 120, rows: 40 }
        })
      })
    )!
  )
  await vi.waitFor(() =>
    expect(messages.some((msg) => JSON.parse(msg).result?.type === 'subscribed')).toBe(true)
  )
  return {
    messages,
    eventsForStream: (streamId: number) =>
      messages
        .map((msg) => JSON.parse(msg).result as { type?: string; streamId?: number })
        .filter((result) => result?.streamId === streamId)
  }
}

describe('terminal.multiplex exit-waiter (#8871)', () => {
  it('surfaces waiter rejection as a stream error, never an exit-shaped end', async () => {
    const exitWaiter: ExitWaiterControl = { resolve: () => {}, reject: () => {} }
    const runtime = stubRuntime(exitWaiter)
    const { eventsForStream } = await subscribeMultiplexStream(runtime)

    exitWaiter.reject(new Error('terminal_handle_stale'))

    await vi.waitFor(() =>
      expect(
        eventsForStream(5).some(
          (event) => event.type === 'error' && event.message === 'terminal_handle_stale'
        )
      ).toBe(true)
    )
    expect(eventsForStream(5).some((event) => event.type === 'end')).toBe(false)
  })

  it('still emits end when the terminal actually exits', async () => {
    const exitWaiter: ExitWaiterControl = { resolve: () => {}, reject: () => {} }
    const runtime = stubRuntime(exitWaiter)
    const { eventsForStream } = await subscribeMultiplexStream(runtime)

    exitWaiter.resolve({
      terminal: 'terminal-1',
      condition: 'exit',
      status: 'exited'
    } as unknown as RuntimeTerminalWait)

    await vi.waitFor(() =>
      expect(eventsForStream(5).some((event) => event.type === 'end')).toBe(true)
    )
    expect(eventsForStream(5).some((event) => event.type === 'error')).toBe(false)
  })
})
