import { describe, expect, it } from 'vitest'

import {
  createAtermWorkerCommandScheduler,
  splitProcessData,
  type PaneRuntimeCommand
} from './aterm-worker-command-scheduler'

// A controllable scheduler harness: `execute` records the order commands actually run,
// and `scheduleDrain` captures the resume so the test drives the yielding drain loop by
// hand (deterministic — no real macrotasks/timers). `now` is a manual clock so the
// slice budget is exercised precisely.
function harness(opts?: { chunkChars?: number; sliceMs?: number; sliceStep?: number }) {
  const executed: PaneRuntimeCommand[] = []
  let pendingResume: (() => void) | null = null
  let clock = 0
  const sliceStep = opts?.sliceStep ?? 0
  const scheduler = createAtermWorkerCommandScheduler({
    execute: (c) => {
      executed.push(c)
      clock += sliceStep // advance the clock per unit so a slice can expire mid-drain
    },
    scheduleDrain: (resume) => {
      pendingResume = resume
    },
    now: () => clock,
    chunkChars: opts?.chunkChars,
    sliceMs: opts?.sliceMs
  })
  // Run one drain slice (mirrors the MessageChannel macrotask firing).
  const runDrain = (): boolean => {
    const resume = pendingResume
    pendingResume = null
    if (!resume) {
      return false
    }
    resume()
    return true
  }
  const drainToIdle = (): void => {
    let guard = 0
    while (runDrain()) {
      if (++guard > 10_000) {
        throw new Error('drain did not converge')
      }
    }
  }
  return { scheduler, executed, runDrain, drainToIdle, setClock: (t: number) => (clock = t) }
}

const proc = (paneId: number, data: string): PaneRuntimeCommand => ({
  type: 'process',
  paneId,
  data
})
const draw = (paneId: number): PaneRuntimeCommand => ({ type: 'draw', paneId })

describe('aterm worker command scheduler (QoS)', () => {
  it('runs focused / non-process commands synchronously on arrival (fast path)', () => {
    const h = harness()
    h.scheduler.noteFocus(1, true)
    h.scheduler.submit(proc(1, 'echo')) // focused process → interactive, sync
    h.scheduler.submit(draw(2)) // background draw (non-process) → cheap, sync
    expect(h.executed.map((c) => c.paneId)).toEqual([1, 2])
    expect(h.scheduler.pendingCount()).toBe(0)
  })

  it('defers a BACKGROUND pane bulk process and services a focused keystroke first', () => {
    const h = harness({ chunkChars: 4 })
    h.scheduler.noteFocus(1, true)
    // Background pane 2 floods (deferred, chunked into 4-char units).
    h.scheduler.submit(proc(2, 'AAAABBBBCCCC'))
    expect(h.executed).toHaveLength(0) // nothing ran synchronously — it deferred
    expect(h.scheduler.pendingCount()).toBe(3)
    // A focused keystroke arrives BEFORE the drain runs → fast-pathed immediately.
    h.scheduler.submit(proc(1, 'x'))
    expect(h.executed.map((c) => c.paneId)).toEqual([1]) // focused echo beat the flood
    // Now drain the background flood.
    h.drainToIdle()
    expect(h.executed.map((c) => c.paneId)).toEqual([1, 2, 2, 2])
    expect(h.executed.slice(1).map((c) => (c as { data: string }).data)).toEqual([
      'AAAA',
      'BBBB',
      'CCCC'
    ])
  })

  it('preserves per-pane FIFO: once a pane has a backlog, its later commands queue behind it', () => {
    const h = harness({ chunkChars: 4 })
    // No focus: pane 5 floods → deferred; a later draw for pane 5 must NOT jump ahead.
    h.scheduler.submit(proc(5, 'AAAABBBB'))
    h.scheduler.submit(draw(5)) // has backlog → queues behind the two process chunks
    h.drainToIdle()
    expect(h.executed.map((c) => c.type)).toEqual(['process', 'process', 'draw'])
  })

  it('time-slices the drain, yielding so a keystroke posted mid-flood is not starved', () => {
    // sliceStep=5, sliceMs=8 → 2 units per slice, then it yields.
    const h = harness({ chunkChars: 1, sliceMs: 8, sliceStep: 5 })
    h.scheduler.submit(proc(9, 'ABCDEF')) // 6 one-char chunks, background
    expect(h.scheduler.pendingCount()).toBe(6)
    h.runDrain() // one slice: ~2 units before the 8ms budget is spent
    const afterFirstSlice = h.executed.length
    expect(afterFirstSlice).toBeGreaterThan(0)
    expect(afterFirstSlice).toBeLessThan(6) // it yielded — did NOT run the whole flood
    h.drainToIdle()
    expect(h.executed).toHaveLength(6)
  })

  it('round-robins background panes so one flood cannot starve its siblings', () => {
    const h = harness({ chunkChars: 1 })
    h.scheduler.submit(proc(1, 'aaaa')) // 4 chunks
    h.scheduler.submit(proc(2, 'b')) // 1 chunk — must not wait for all of pane 1
    h.drainToIdle()
    const firstTwoPanes = h.executed.slice(0, 2).map((c) => c.paneId)
    expect(firstTwoPanes).toContain(2) // pane 2 serviced early, not after all of pane 1
  })

  it('forget() drops a pane deferred work so nothing runs against a freed engine', () => {
    const h = harness({ chunkChars: 1 })
    h.scheduler.submit(proc(7, 'abcd'))
    expect(h.scheduler.pendingCount()).toBe(4)
    h.scheduler.forget(7)
    expect(h.scheduler.pendingCount()).toBe(0)
    h.drainToIdle()
    expect(h.executed).toHaveLength(0)
  })

  it('splitProcessData never severs a surrogate pair', () => {
    // A 4-code-unit string of two astral glyphs (each a surrogate pair) split at 3.
    const emoji = '\u{1F600}\u{1F601}' // 4 UTF-16 code units
    const parts = splitProcessData(emoji, 3)
    // No part may start or end on a lone surrogate half.
    for (const part of parts) {
      expect(part).toBe(part) // decodes cleanly
      expect([...part].every((ch) => ch.codePointAt(0)! <= 0x10ffff)).toBe(true)
    }
    expect(parts.join('')).toBe(emoji)
  })
})
