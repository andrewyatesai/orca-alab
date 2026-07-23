// Fed §2.4 remote wire, host side: terminal.search / terminal.searchContext
// responses, the anchor echo gated on (anchorGen, emulator incarnation), and
// the snapshot reply carrying hostRowAnchor/anchorGen for the multiplex
// subscribe path — the snapshot the remote client actually replays.
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { RpcDispatcher } from './dispatcher'
import type { RpcRequest } from './core'
import type { OrcaRuntimeService } from '../orca-runtime'
import { TERMINAL_METHODS } from './methods/terminal'
import type { RuntimeTerminalWait } from '../../../shared/runtime-types'
import {
  TerminalStreamOpcode,
  decodeTerminalStreamFrame,
  decodeTerminalStreamJson,
  encodeTerminalStreamFrame,
  encodeTerminalStreamJson
} from '../../../shared/terminal-stream-protocol'
import {
  resetTerminalHostRowAnchorLedgerForTest,
  terminalHostRowAnchorLedger
} from '../terminal-host-row-anchor'
import type { RemoteTerminalSearchResult } from '../../../shared/terminal-remote-search-protocol'

const SEARCH_RESULT = {
  available: true,
  matches: [{ hostRow: 137, col: 2, len: 6, line: 'a needle row' }],
  total: 3,
  incomplete: true,
  originRow: 90,
  hostCols: 80,
  alternateScreen: false,
  incarnation: 7
}

function stubRuntime(overrides: Partial<OrcaRuntimeService> = {}): OrcaRuntimeService {
  const cleanups = new Map<string, () => void>()
  return {
    getRuntimeId: () => 'test-runtime',
    resolveLeafForHandle: vi.fn().mockReturnValue({ ptyId: 'pty-1' }),
    resolveLiveLeafForHandle: vi.fn().mockReturnValue({ ptyId: 'pty-1' }),
    searchTerminalScrollback: vi.fn().mockResolvedValue(SEARCH_RESULT),
    terminalSearchContext: vi.fn().mockResolvedValue({
      available: true,
      lines: ['above', 'match', 'below'],
      firstHostRow: 136,
      incarnation: 7
    }),
    readTerminal: vi.fn().mockResolvedValue({ tail: [], truncated: false }),
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
    handleMobileSubscribe: vi.fn().mockResolvedValue(undefined),
    handleMobileUnsubscribe: vi.fn(),
    waitForTerminal: vi.fn(() => new Promise<RuntimeTerminalWait>(() => {})),
    updateDesktopViewport: vi.fn().mockResolvedValue(true),
    ...overrides
  } as OrcaRuntimeService
}

function makeRequest(method: string, params?: unknown): RpcRequest {
  return { id: 'req-1', authToken: 'tok', method, params }
}

beforeEach(() => {
  resetTerminalHostRowAnchorLedgerForTest()
})

describe('terminal.search', () => {
  it('returns schema-versioned match summaries with the gen echo', async () => {
    const runtime = stubRuntime()
    const dispatcher = new RpcDispatcher({ runtime, methods: TERMINAL_METHODS })
    const response = await dispatcher.dispatch(
      makeRequest('terminal.search', { terminal: 't-1', query: 'needle', gen: 12 })
    )
    expect(response.ok).toBe(true)
    const result = (response as { result: RemoteTerminalSearchResult }).result
    expect(result.searchSchema).toBe(1)
    expect(result.available).toBe(true)
    expect(result.matches).toEqual(SEARCH_RESULT.matches)
    expect(result.total).toBe(3)
    expect(result.incomplete).toBe(true)
    expect(result.gen).toBe(12)
    expect(result.hostCols).toBe(80)
    // No clientAnchorGen in the request → no anchor echo.
    expect(result.hostRowAnchor).toBeUndefined()
    expect(result.anchorGen).toBeUndefined()
  })

  it('echoes the anchor ONLY for a ledger-known gen minted by the live emulator incarnation', async () => {
    const gen = terminalHostRowAnchorLedger().mint('pty-1', {
      hostRowAnchor: 100,
      incarnation: 7,
      hostCols: 120
    })
    const runtime = stubRuntime()
    const dispatcher = new RpcDispatcher({ runtime, methods: TERMINAL_METHODS })
    const response = await dispatcher.dispatch(
      makeRequest('terminal.search', { terminal: 't-1', query: 'needle', clientAnchorGen: gen })
    )
    const result = (response as { result: RemoteTerminalSearchResult }).result
    expect(result.hostRowAnchor).toBe(100)
    expect(result.anchorGen).toBe(gen)
    expect(result.anchorHostCols).toBe(120)
  })

  it('withholds the anchor when the gen is unknown or minted by a dead emulator incarnation', async () => {
    const staleGen = terminalHostRowAnchorLedger().mint('pty-1', {
      hostRowAnchor: 100,
      incarnation: 6, // current is 7 → coordinates restarted since this anchor
      hostCols: 80
    })
    const runtime = stubRuntime()
    const dispatcher = new RpcDispatcher({ runtime, methods: TERMINAL_METHODS })
    for (const clientAnchorGen of [staleGen, 424242]) {
      const response = await dispatcher.dispatch(
        makeRequest('terminal.search', { terminal: 't-1', query: 'needle', clientAnchorGen })
      )
      const result = (response as { result: RemoteTerminalSearchResult }).result
      expect(result.available).toBe(true)
      expect(result.hostRowAnchor).toBeUndefined()
      expect(result.anchorGen).toBeUndefined()
    }
  })

  it('reports available:false verbatim so old panes degrade per-pane, not per-query', async () => {
    const runtime = stubRuntime({
      searchTerminalScrollback: vi.fn().mockResolvedValue({
        available: false,
        matches: [],
        total: 0,
        incomplete: false,
        originRow: null,
        hostCols: null,
        alternateScreen: false,
        incarnation: null
      })
    })
    const dispatcher = new RpcDispatcher({ runtime, methods: TERMINAL_METHODS })
    const response = await dispatcher.dispatch(
      makeRequest('terminal.search', { terminal: 't-1', query: 'needle' })
    )
    expect(response.ok).toBe(true)
    const result = (response as { result: RemoteTerminalSearchResult }).result
    expect(result.available).toBe(false)
    expect(result.matches).toEqual([])
  })

  it('refuses an already-aborted request without running the scan (relay abort path)', async () => {
    const search = vi.fn()
    const runtime = stubRuntime({ searchTerminalScrollback: search })
    const dispatcher = new RpcDispatcher({ runtime, methods: TERMINAL_METHODS })
    const controller = new AbortController()
    controller.abort()
    const response = await dispatcher.dispatch(
      makeRequest('terminal.search', { terminal: 't-1', query: 'needle' }),
      { signal: controller.signal }
    )
    expect(response.ok).toBe(false)
    expect(search).not.toHaveBeenCalled()
  })
})

describe('terminal.searchContext', () => {
  it('returns the context window with stable first row', async () => {
    const runtime = stubRuntime()
    const dispatcher = new RpcDispatcher({ runtime, methods: TERMINAL_METHODS })
    const response = await dispatcher.dispatch(
      makeRequest('terminal.searchContext', { terminal: 't-1', hostRow: 137, before: 1, after: 1 })
    )
    expect(response.ok).toBe(true)
    const result = (
      response as { result: { available: boolean; lines: string[]; firstHostRow: number } }
    ).result
    expect(result.available).toBe(true)
    expect(result.lines).toEqual(['above', 'match', 'below'])
    expect(result.firstHostRow).toBe(136)
    expect(runtime.terminalSearchContext).toHaveBeenCalledWith('t-1', {
      hostRow: 137,
      before: 1,
      after: 1
    })
  })
})

describe('multiplex subscribe snapshot anchor (the snapshot-reply anchor)', () => {
  async function runSubscribe(runtime: OrcaRuntimeService): Promise<Record<string, unknown>> {
    const messages: string[] = []
    const binaryFrames: Uint8Array<ArrayBufferLike>[] = []
    const handlers = new Map<
      number,
      (frame: NonNullable<ReturnType<typeof decodeTerminalStreamFrame>>) => void
    >()
    const dispatcher = new RpcDispatcher({ runtime, methods: TERMINAL_METHODS })
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
    const decoded = binaryFrames.map((frame) => decodeTerminalStreamFrame(frame))
    const snapshotStart = decoded.find(
      (frame) => frame?.opcode === TerminalStreamOpcode.SnapshotStart
    )!
    const info = decodeTerminalStreamJson<Record<string, unknown>>(snapshotStart.payload)!
    runtime.cleanupSubscription('terminal-multiplex:conn-1')
    await dispatchPromise
    return info
  }

  it('stamps hostRowAnchor/anchorGen from the headless origin and records them in the ledger', async () => {
    vi.useFakeTimers()
    try {
      const runtime = stubRuntime({
        serializeTerminalBuffer: vi.fn().mockResolvedValue({
          data: '\x1b[2J\x1b[HGRID',
          scrollbackAnsi: 'old line\r\nnewer line\r\n',
          cols: 80,
          rows: 24,
          seq: 42,
          source: 'headless',
          scrollbackLines: 10,
          retainedOriginRow: 90,
          headlessIncarnation: 7
        })
      })
      const info = await runSubscribe(runtime)
      // 2 of 10 history rows fit in the wire payload: first serialized row is
      // origin(90) + history(10) − kept(2) = 98.
      expect(info.hostRowAnchor).toBe(98)
      expect(typeof info.anchorGen).toBe('number')
      const record = terminalHostRowAnchorLedger().lookup('pty-1', info.anchorGen as number, 7)
      expect(record).toMatchObject({ hostRowAnchor: 98, hostCols: 80 })
    } finally {
      vi.useRealTimers()
    }
  })

  it('omits the anchor for renderer-source and alternate-screen snapshots', async () => {
    vi.useFakeTimers()
    try {
      for (const serialized of [
        { source: 'renderer' as const },
        { source: 'headless' as const, alternateScreen: true }
      ]) {
        resetTerminalHostRowAnchorLedgerForTest()
        const runtime = stubRuntime({
          serializeTerminalBuffer: vi.fn().mockResolvedValue({
            data: 'GRID',
            scrollbackAnsi: 'line\r\n',
            cols: 80,
            rows: 24,
            seq: 42,
            scrollbackLines: 1,
            retainedOriginRow: 0,
            headlessIncarnation: 7,
            ...serialized
          })
        })
        const info = await runSubscribe(runtime)
        expect(info.hostRowAnchor).toBeUndefined()
        expect(info.anchorGen).toBeUndefined()
      }
    } finally {
      vi.useRealTimers()
    }
  })
})
