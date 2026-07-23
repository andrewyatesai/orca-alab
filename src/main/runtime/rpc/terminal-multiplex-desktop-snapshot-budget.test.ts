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
import { MOBILE_SNAPSHOT_BYTE_BUDGET } from '../scrollback-limits'

// ssh-pane-parking.md §3.3: the desktop initial subscribe snapshot was
// UNBUDGETED (full scrollbackAnsi + grid, truncatedByByteBudget hardcoded
// false) while the client multiplexer hard-drops any snapshot over its 2 MiB
// replay limit — a parked output-heavy pane would reveal empty on every
// reveal. The desktop subscribe branch must bound history under the same
// 2 MiB budget the requested path uses, cut on a line boundary, and report
// truncatedByByteBudget honestly.

const REQUESTED_SNAPSHOT_BYTE_BUDGET = 2 * 1024 * 1024
// 1 KiB per line including the CRLF terminator, so byte math is exact.
const LINE_BODY = 'a'.repeat(1022)
const GRID_FRAME = '\x1b[2J\x1b[HGRID'

function stubRuntime(overrides: Partial<OrcaRuntimeService> = {}): OrcaRuntimeService {
  const cleanups = new Map<string, () => void>()
  return {
    getRuntimeId: () => 'test-runtime',
    resolveLiveLeafForHandle: vi.fn().mockReturnValue({ ptyId: 'pty-1' }),
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

async function runSubscribe(
  runtime: OrcaRuntimeService,
  client: { id: string; type: string }
): Promise<{
  snapshotInfo: Record<string, unknown>
  snapshotData: string
  finish: () => Promise<void>
}> {
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
  expect(messages.some((msg) => JSON.parse(msg).result?.type === 'ready')).toBe(true)

  handlers.get(0)?.(
    decodeTerminalStreamFrame(
      encodeTerminalStreamFrame({
        opcode: TerminalStreamOpcode.Subscribe,
        streamId: 0,
        seq: 1,
        payload: encodeTerminalStreamJson({ streamId: 5, terminal: 'terminal-1', client })
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
  const snapshotInfo = decodeTerminalStreamJson<Record<string, unknown>>(snapshotStart.payload)!
  const snapshotData = decoded
    .filter((frame) => frame?.opcode === TerminalStreamOpcode.SnapshotChunk)
    .map((frame) => decodeTerminalStreamText(frame!.payload))
    .join('')
  return {
    snapshotInfo,
    snapshotData,
    finish: async () => {
      runtime.cleanupSubscription('terminal-multiplex:conn-1')
      await dispatchPromise
    }
  }
}

describe('terminal.multiplex desktop subscribe snapshot budget', () => {
  it('bounds an oversized desktop subscribe snapshot to the 2 MiB budget on a line boundary', async () => {
    vi.useFakeTimers()
    try {
      const lineCount = 3000
      const scrollbackAnsi = Array.from(
        { length: lineCount },
        (_, index) => `${String(index % 10)}${LINE_BODY.slice(1)}\r\n`
      ).join('')
      const runtime = stubRuntime({
        serializeTerminalBuffer: vi.fn().mockResolvedValue({
          data: GRID_FRAME,
          scrollbackAnsi,
          cols: 80,
          rows: 24,
          seq: 42
        })
      })

      const { snapshotInfo, snapshotData, finish } = await runSubscribe(runtime, {
        id: 'desktop-1',
        type: 'desktop'
      })

      expect(Buffer.byteLength(snapshotData, 'utf8')).toBeLessThanOrEqual(
        REQUESTED_SNAPSHOT_BYTE_BUDGET
      )
      expect(snapshotData.endsWith(GRID_FRAME)).toBe(true)
      const keptScrollback = snapshotData.slice(0, snapshotData.length - GRID_FRAME.length)
      // Line-boundary cut: the kept history is a whole-line suffix of the original.
      expect(keptScrollback.length % 1024).toBe(0)
      expect(scrollbackAnsi.endsWith(keptScrollback)).toBe(true)
      // The newest lines survive, the oldest are dropped.
      const keptLines = keptScrollback.length / 1024
      expect(keptLines).toBeGreaterThan(0)
      expect(keptLines).toBeLessThan(lineCount)
      expect(snapshotInfo.truncatedByByteBudget).toBe(true)
      await finish()
    } finally {
      vi.useRealTimers()
    }
  })

  it('keeps a within-budget desktop snapshot whole with an honest truncatedByByteBudget=false', async () => {
    vi.useFakeTimers()
    try {
      const scrollbackAnsi = 'small history line\r\n'.repeat(10)
      const runtime = stubRuntime({
        serializeTerminalBuffer: vi.fn().mockResolvedValue({
          data: GRID_FRAME,
          scrollbackAnsi,
          cols: 80,
          rows: 24,
          seq: 42
        })
      })

      const { snapshotInfo, snapshotData, finish } = await runSubscribe(runtime, {
        id: 'desktop-1',
        type: 'desktop'
      })

      expect(snapshotData).toBe(scrollbackAnsi + GRID_FRAME)
      expect(snapshotInfo.truncatedByByteBudget).toBe(false)
      await finish()
    } finally {
      vi.useRealTimers()
    }
  })

  it('keeps the mobile subscribe path on the mobile row cap and byte budget', async () => {
    vi.useFakeTimers()
    try {
      // 2000 short lines: over the 1000-row mobile cap, under every byte budget.
      const scrollbackAnsi = Array.from({ length: 2000 }, (_, i) => `m${i}\r\n`).join('')
      const runtime = stubRuntime({
        serializeTerminalBuffer: vi.fn().mockResolvedValue({
          data: GRID_FRAME,
          scrollbackAnsi,
          cols: 80,
          rows: 24,
          seq: 42
        })
      })

      const { snapshotInfo, snapshotData, finish } = await runSubscribe(runtime, {
        id: 'mobile-1',
        type: 'mobile'
      })

      const keptScrollback = snapshotData.slice(0, snapshotData.length - GRID_FRAME.length)
      const keptLines = keptScrollback.split('\r\n').filter((line) => line.length > 0)
      // Row-cap trim only — the byte-budget flag stays honest (mobile budget untouched).
      expect(keptLines.length).toBe(1000)
      expect(keptLines[0]).toBe('m1000')
      expect(keptLines.at(-1)).toBe('m1999')
      expect(Buffer.byteLength(snapshotData, 'utf8')).toBeLessThanOrEqual(
        MOBILE_SNAPSHOT_BYTE_BUDGET
      )
      expect(snapshotInfo.truncatedByByteBudget).toBe(false)
      await finish()
    } finally {
      vi.useRealTimers()
    }
  })
})
