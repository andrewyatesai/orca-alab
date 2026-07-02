import { describe, expect, it } from 'vitest'
import type { ManagedPane } from '@/lib/pane-manager/pane-manager'
import {
  isPaneReplaying,
  replayIntoTerminal,
  replayIntoTerminalAsync,
  type ReplayingPanesRef
} from './replay-guard'

function makeRef(): ReplayingPanesRef {
  return { current: new Map() } as ReplayingPanesRef
}

type FakeTerminal = {
  write: (data: string, cb?: () => void) => void
  lastData: string[]
  pendingCallbacks: (() => void)[]
  rows: number
  buffer: {
    active: {
      baseY: number
      viewportY: number
    }
  }
  _core: {
    refresh: (start: number, end: number, sync?: boolean) => void
  }
  /** Flush all pending xterm write callbacks, simulating parse completion. */
  flush: () => void
}

function makeFakePane(paneId: number): { pane: ManagedPane; terminal: FakeTerminal } {
  const pendingCallbacks: (() => void)[] = []
  const terminal: FakeTerminal = {
    lastData: [],
    pendingCallbacks,
    rows: 24,
    buffer: {
      active: {
        baseY: 0,
        viewportY: 0
      }
    },
    _core: {
      refresh() {}
    },
    write(data: string, cb?: () => void) {
      terminal.lastData.push(data)
      if (cb) {
        pendingCallbacks.push(cb)
      }
    },
    flush() {
      while (pendingCallbacks.length > 0) {
        pendingCallbacks.shift()!()
      }
    }
  }
  // Only `id` and `terminal` are exercised by replayIntoTerminal.
  const pane = { id: paneId, terminal } as unknown as ManagedPane
  return { pane, terminal }
}

/** Attach a fake ASYNC controller (the worker engine path): settle() resolves only
 *  when the test releases it, mimicking the worker's later-task parse completion. */
function attachAsyncController(pane: ManagedPane): { settleEngine: () => void } {
  let resolveSettle: () => void = () => undefined
  const settled = new Promise<void>((resolve) => {
    resolveSettle = resolve
  })
  ;(pane as { atermController?: unknown }).atermController = {
    settle: () => settled
  }
  return { settleEngine: resolveSettle }
}

const drainMicrotasks = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0))

describe('replay-guard', () => {
  it('reports no replay for untouched pane', () => {
    const ref = makeRef()
    expect(isPaneReplaying(ref, 1)).toBe(false)
  })

  it('is replaying between write dispatch and xterm parse completion', () => {
    const ref = makeRef()
    const { pane, terminal } = makeFakePane(1)

    replayIntoTerminal(pane, ref, 'hello')

    // Before xterm fires its write-completion callback, the guard is engaged —
    // this is the window during which xterm could emit auto-replies for any
    // query sequences embedded in the replayed data.
    expect(isPaneReplaying(ref, 1)).toBe(true)

    terminal.flush()
    expect(isPaneReplaying(ref, 1)).toBe(false)
  })

  it('composes nested replays via a counter', () => {
    const ref = makeRef()
    const { pane, terminal } = makeFakePane(1)

    // Simulates the cold-restore path: clear preamble + scrollback + banner
    // dispatched back-to-back before xterm completes any of them.
    replayIntoTerminal(pane, ref, '\x1b[2J\x1b[3J\x1b[H')
    replayIntoTerminal(pane, ref, 'scrollback bytes')
    replayIntoTerminal(pane, ref, '--- session restored ---')
    expect(isPaneReplaying(ref, 1)).toBe(true)

    // Completion of the first write must not clear the guard — the later
    // writes are still in xterm's queue and may still auto-reply.
    terminal.pendingCallbacks.shift()!()
    expect(isPaneReplaying(ref, 1)).toBe(true)

    terminal.pendingCallbacks.shift()!()
    expect(isPaneReplaying(ref, 1)).toBe(true)

    terminal.pendingCallbacks.shift()!()
    expect(isPaneReplaying(ref, 1)).toBe(false)
  })

  it('keeps each pane independent', () => {
    const ref = makeRef()
    const a = makeFakePane(1)
    const b = makeFakePane(2)

    replayIntoTerminal(a.pane, ref, 'a')
    expect(isPaneReplaying(ref, 1)).toBe(true)
    expect(isPaneReplaying(ref, 2)).toBe(false)

    replayIntoTerminal(b.pane, ref, 'b')
    expect(isPaneReplaying(ref, 2)).toBe(true)

    a.terminal.flush()
    expect(isPaneReplaying(ref, 1)).toBe(false)
    expect(isPaneReplaying(ref, 2)).toBe(true)

    b.terminal.flush()
    expect(isPaneReplaying(ref, 2)).toBe(false)
  })

  it('skips empty data without touching the guard or xterm', () => {
    const ref = makeRef()
    const { pane, terminal } = makeFakePane(1)
    replayIntoTerminal(pane, ref, '')
    expect(terminal.lastData).toEqual([])
    expect(isPaneReplaying(ref, 1)).toBe(false)
  })

  it('removes the counter entry when the last replay completes', () => {
    const ref = makeRef()
    const { pane, terminal } = makeFakePane(1)
    replayIntoTerminal(pane, ref, 'x')
    terminal.flush()
    expect(ref.current.has(1)).toBe(false)
  })

  it('holds the guard open past the write ack until the async engine settles', async () => {
    const ref = makeRef()
    const { pane, terminal } = makeFakePane(1)
    const { settleEngine } = attachAsyncController(pane)

    // Replayed bytes embedding a DA1 query the engine will auto-reply to.
    replayIntoTerminal(pane, ref, '\x1b[c')
    terminal.flush()
    await drainMicrotasks()

    // The write callback fired, but the worker engine hasn't parsed the replayed
    // bytes yet — a DA/CPR reply arriving NOW must still be dropped, so the guard
    // stays engaged until settle() resolves.
    expect(isPaneReplaying(ref, 1)).toBe(true)

    settleEngine()
    await drainMicrotasks()
    expect(isPaneReplaying(ref, 1)).toBe(false)
    expect(ref.current.has(1)).toBe(false)
  })

  it('resolves replayIntoTerminalAsync only after the engine settles', async () => {
    const ref = makeRef()
    const { pane, terminal } = makeFakePane(1)
    const { settleEngine } = attachAsyncController(pane)

    let resolved = false
    const done = replayIntoTerminalAsync(pane, ref, 'scrollback').then(() => {
      resolved = true
    })
    terminal.flush()
    await drainMicrotasks()
    expect(resolved).toBe(false)
    expect(isPaneReplaying(ref, 1)).toBe(true)

    settleEngine()
    await done
    expect(isPaneReplaying(ref, 1)).toBe(false)
  })

  it('releases synchronously when no controller is attached (pre-attach replay)', () => {
    const ref = makeRef()
    const { pane, terminal } = makeFakePane(1)
    replayIntoTerminal(pane, ref, 'x')
    terminal.flush()
    // No controller → no settle fence; the sync decrement contract is unchanged.
    expect(isPaneReplaying(ref, 1)).toBe(false)
  })

  it('schedules a follow-up repaint for replayed cursor restores', () => {
    const scheduledFrames: FrameRequestCallback[] = []
    const originalRequestAnimationFrame = globalThis.requestAnimationFrame
    const originalCancelAnimationFrame = globalThis.cancelAnimationFrame
    globalThis.requestAnimationFrame = ((callback: FrameRequestCallback) => {
      scheduledFrames.push(callback)
      return scheduledFrames.length
    }) as typeof requestAnimationFrame
    globalThis.cancelAnimationFrame = (() => {}) as typeof cancelAnimationFrame

    try {
      const ref = makeRef()
      const { pane, terminal } = makeFakePane(1)
      let refreshCount = 0
      terminal._core.refresh = () => {
        refreshCount += 1
      }

      replayIntoTerminal(pane, ref, '\x1b[?25h')
      terminal.flush()

      expect(refreshCount).toBe(1)
      expect(scheduledFrames).toHaveLength(1)

      scheduledFrames[0]?.(16)

      expect(refreshCount).toBe(2)
    } finally {
      globalThis.requestAnimationFrame = originalRequestAnimationFrame
      globalThis.cancelAnimationFrame = originalCancelAnimationFrame
    }
  })
})
