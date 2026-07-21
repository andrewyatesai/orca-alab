/** #9490: mobile subscribers get a wider output flush window so bursty output wakes the phone's radio less often. */
import { afterEach, describe, expect, it, vi } from 'vitest'
import { RpcDispatcher } from './dispatcher'
import type { RpcRequest } from './core'
import type { OrcaRuntimeService } from '../orca-runtime'
import { TERMINAL_METHODS } from './methods/terminal'
import {
  TerminalStreamOpcode,
  decodeTerminalStreamFrame,
  decodeTerminalStreamText
} from '../../../shared/terminal-stream-protocol'
import type { RuntimeTerminalWait } from '../../../shared/runtime-types'

type TerminalDataListener = (data: string, meta?: { seq?: number }) => void

function makeSubscribeHarness(): {
  runtime: OrcaRuntimeService
  sendBinary: ReturnType<typeof vi.fn<(bytes: Uint8Array) => void>>
  emit: ReturnType<typeof vi.fn<(response: string) => void>>
  getDataListener: () => TerminalDataListener | null
  endStream: (subscriptionId: string) => void
} {
  const cleanups = new Map<string, () => void>()
  let dataListener: TerminalDataListener | null = null
  const sendBinary = vi.fn<(bytes: Uint8Array) => void>()
  const emit = vi.fn<(response: string) => void>()
  const runtime = {
    getRuntimeId: () => 'test-runtime',
    registerRemoteTerminalViewSubscriber: () => () => {},
    requestRendererTerminalTabMount: () => false,
    getRendererTerminalSerializerGenerationForHandle: () => 0,
    getRendererTerminalSerializerGeneration: () => 0,
    waitForRendererTerminalSerializer: async () => false,
    getPtyOutputSequence: () => 0,
    replaceHeadlessTerminalFromRendererSnapshotForRecovery: () => {},
    serializeRendererTerminalBuffer: async () => null,
    hasHeadlessTerminalState: () => true,
    resolveLeafForHandle: () => ({ ptyId: 'pty-1' }),
    handleMobileSubscribe: vi.fn().mockResolvedValue(true),
    handleMobileUnsubscribe: vi.fn(),
    subscribeToTerminalData: vi.fn((_ptyId: string, listener: TerminalDataListener) => {
      dataListener = listener
      return vi.fn()
    }),
    readTerminal: vi.fn().mockResolvedValue({ tail: [], truncated: false }),
    serializeTerminalBuffer: vi.fn().mockResolvedValue({ data: '$ ', cols: 80, rows: 24, seq: 0 }),
    getTerminalSize: vi.fn().mockReturnValue({ cols: 80, rows: 24 }),
    getMobileDisplayMode: vi.fn().mockReturnValue('auto'),
    getLayout: vi.fn().mockReturnValue({ seq: 1 }),
    isTerminalAlternateScreen: vi.fn().mockReturnValue(false),
    subscribeToTerminalResize: vi.fn().mockReturnValue(vi.fn()),
    subscribeToFitOverrideChanges: vi.fn().mockReturnValue(vi.fn()),
    registerSubscriptionCleanup: vi.fn((id: string, cleanup: () => void) => {
      cleanups.set(id, cleanup)
    }),
    cleanupSubscription: vi.fn((id: string) => {
      cleanups.get(id)?.()
      cleanups.delete(id)
    }),
    waitForTerminal: vi.fn(() => new Promise<RuntimeTerminalWait>(() => {}))
  } as unknown as OrcaRuntimeService
  return {
    runtime,
    sendBinary,
    emit,
    getDataListener: () => dataListener,
    endStream: (subscriptionId: string) => runtime.cleanupSubscription(subscriptionId)
  }
}

const makeRequest = (clientType: 'mobile' | 'desktop'): RpcRequest => ({
  id: 'req-1',
  authToken: 'tok',
  method: 'terminal.subscribe',
  params: {
    terminal: 'terminal-1',
    client: { id: `${clientType}-1`, type: clientType },
    capabilities: { terminalBinaryStream: 1 }
  }
})

function decodedOutputText(
  sendBinary: ReturnType<typeof vi.fn<(bytes: Uint8Array) => void>>
): string {
  return sendBinary.mock.calls
    .map(([bytes]) => decodeTerminalStreamFrame(bytes))
    .filter((frame) => frame?.opcode === TerminalStreamOpcode.Output)
    .map((frame) => decodeTerminalStreamText(frame!.payload))
    .join('')
}

async function subscribeAndGetListener(
  harness: ReturnType<typeof makeSubscribeHarness>,
  clientType: 'mobile' | 'desktop'
): Promise<{
  listener: TerminalDataListener
  dispatchPromise: Promise<unknown>
}> {
  const dispatcher = new RpcDispatcher({ runtime: harness.runtime, methods: TERMINAL_METHODS })
  const dispatchPromise = dispatcher.dispatchStreaming(makeRequest(clientType), harness.emit, {
    connectionId: `conn-${clientType}`,
    sendBinary: harness.sendBinary,
    registerBinaryStreamHandler: vi.fn(() => vi.fn())
  })
  // Why: settle the subscribe's promise chain (snapshot/read) without waiting on real time.
  await vi.advanceTimersByTimeAsync(0)
  // Why: the dispatcher serializes stream events to JSON before emitting.
  expect(
    harness.emit.mock.calls.some(
      ([payload]) => typeof payload === 'string' && payload.includes('"type":"subscribed"')
    )
  ).toBe(true)
  const listener = harness.getDataListener()
  if (!listener) {
    throw new Error('expected terminal data listener')
  }
  harness.sendBinary.mockClear()
  return { listener, dispatchPromise }
}

describe('terminal.subscribe output flush window (#9490)', () => {
  afterEach(() => {
    vi.useRealTimers()
  })

  it('holds mobile output for the wider 50ms flush window to batch radio wakeups', async () => {
    vi.useFakeTimers()
    const harness = makeSubscribeHarness()
    const { listener, dispatchPromise } = await subscribeAndGetListener(harness, 'mobile')

    listener('chunk-1', { seq: 1 })
    await vi.advanceTimersByTimeAsync(5)
    // Why: the desktop window (5ms) must NOT flush a mobile stream.
    expect(decodedOutputText(harness.sendBinary)).toBe('')

    listener('chunk-2', { seq: 2 })
    await vi.advanceTimersByTimeAsync(45)
    expect(decodedOutputText(harness.sendBinary)).toBe('chunk-1chunk-2')
    expect(
      harness.sendBinary.mock.calls
        .map(([bytes]) => decodeTerminalStreamFrame(bytes))
        .filter((frame) => frame?.opcode === TerminalStreamOpcode.Output)
    ).toHaveLength(1)

    harness.endStream('terminal-1:mobile-1')
    await dispatchPromise
  })

  it('keeps the tight 5ms flush window for desktop binary-stream subscribers', async () => {
    vi.useFakeTimers()
    const harness = makeSubscribeHarness()
    const { listener, dispatchPromise } = await subscribeAndGetListener(harness, 'desktop')

    listener('chunk-1', { seq: 1 })
    await vi.advanceTimersByTimeAsync(5)
    expect(decodedOutputText(harness.sendBinary)).toBe('chunk-1')

    harness.endStream('terminal-1:desktop-1')
    await dispatchPromise
  })
})
