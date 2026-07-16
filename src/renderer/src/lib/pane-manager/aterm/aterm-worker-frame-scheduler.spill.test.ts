import { describe, expect, it } from 'vitest'
import type { AtermWorkerState } from './aterm-render-worker-protocol'
import {
  createSharedWorkerRafLoop,
  createWorkerFrameScheduler
} from './aterm-worker-frame-scheduler'

// Plan risk 5 — the subtlest scheduling seam: worker presentNow/postNow paint
// OUTSIDE the shared-rAF flush, so the spill pass runs from BOTH the flush
// epilogue AND the eager-paint tail. The swap-cleared dirty array must make
// that pair yield exactly ONE real pass per painted state — never zero (a
// lagged ring) and never two (double compositing work).

type SchedulerTerm = {
  render: () => void
  tickEffects: () => boolean
  effectsIdleDeadlineMs: () => number | undefined
  advanceEffectsBy: (dtMs: number) => void
  buildState: () => AtermWorkerState
  dimensions: () => { cols: number; rows: number; cellWidth: number; cellHeight: number }
}

function makeTerm(): { term: SchedulerTerm; renders: () => number } {
  let renders = 0
  const term: SchedulerTerm = {
    render: () => {
      renders++
    },
    tickEffects: () => false,
    effectsIdleDeadlineMs: () => undefined,
    advanceEffectsBy: () => undefined,
    buildState: () =>
      ({ cols: 80, rows: 24, cellWidth: 8, cellHeight: 16 }) as unknown as AtermWorkerState,
    dimensions: () => ({ cols: 80, rows: 24, cellWidth: 8, cellHeight: 16 })
  }
  return { term, renders: () => renders }
}

/** A spill stub with the compositor's swap-clear semantics: marks accumulate a
 *  dirty flag; a pass with the flag set is a REAL pass and consumes it. */
function makeSpill(): {
  spill: { markPaneDirty: () => void; runPassNow: () => void }
  realPasses: () => number
  passInvocations: () => number
} {
  let dirty = false
  let realPasses = 0
  let passInvocations = 0
  return {
    spill: {
      markPaneDirty: () => {
        dirty = true
      },
      runPassNow: () => {
        passInvocations++
        if (dirty) {
          dirty = false
          realPasses++
        }
      }
    },
    realPasses: () => realPasses,
    passInvocations: () => passInvocations
  }
}

function makeHarness(): {
  scheduler: ReturnType<typeof createWorkerFrameScheduler>
  flushFrame: () => void
  renders: () => number
  realPasses: () => number
  passInvocations: () => number
} {
  const rafQueue: (() => void)[] = []
  const { spill, realPasses, passInvocations } = makeSpill()
  // The shared loop's flush epilogue is the SAME pass the eager tail runs.
  const sharedRaf = createSharedWorkerRafLoop(
    (cb) => {
      rafQueue.push(cb)
    },
    () => spill.runPassNow()
  )
  const { term, renders } = makeTerm()
  const scheduler = createWorkerFrameScheduler({
    getTerm: () => term,
    post: () => undefined,
    raf: sharedRaf,
    spill
  })
  return {
    scheduler,
    flushFrame: () => {
      // One rendering frame: run every armed rAF callback (the shared loop
      // enqueues ONE flush per frame; flushing it runs panes + the epilogue).
      const run = rafQueue.splice(0, rafQueue.length)
      for (const cb of run) {
        cb()
      }
    },
    renders,
    realPasses,
    passInvocations
  }
}

describe('worker frame scheduler spill hooks (risk 5: eager paints outside the flush)', () => {
  it('a scheduled draw inside the flush runs ONE real pass via the epilogue', () => {
    const h = makeHarness()
    h.scheduler.schedule()
    expect(h.renders()).toBe(0)
    h.flushFrame()
    expect(h.renders()).toBe(1)
    expect(h.realPasses()).toBe(1)
  })

  it('presentNow BEFORE the armed flush: the eager tail passes; the epilogue is a no-op', () => {
    const h = makeHarness()
    h.scheduler.schedule() // arms the shared-rAF flush
    h.scheduler.presentNow() // eager echo paint, synchronous, outside the flush
    // The paint + its spill pass landed in the SAME task (no one-flush ring lag).
    expect(h.renders()).toBe(1)
    expect(h.realPasses()).toBe(1)
    h.flushFrame() // the armed flush: draw already consumed → epilogue no-ops
    expect(h.renders()).toBe(1)
    expect(h.passInvocations()).toBeGreaterThanOrEqual(2)
    expect(h.realPasses()).toBe(1)
  })

  it('presentNow AFTER a flushed frame in the same frame window still passes exactly once', () => {
    const h = makeHarness()
    h.scheduler.schedule()
    h.flushFrame()
    expect(h.realPasses()).toBe(1)
    h.scheduler.presentNow() // new engine state, eager paint
    expect(h.renders()).toBe(2)
    expect(h.realPasses()).toBe(2)
    // Its gate-reopen rAF flush finds nothing dirty.
    h.flushFrame()
    expect(h.realPasses()).toBe(2)
  })

  it('postNow (the first-frame synchronous post) runs the tail pass', () => {
    const h = makeHarness()
    h.scheduler.postNow()
    expect(h.renders()).toBe(1)
    expect(h.realPasses()).toBe(1)
  })

  it('suspended panes never mark or pass', () => {
    const h = makeHarness()
    h.scheduler.setSuspended(true)
    h.scheduler.presentNow() // falls back to schedule(); suspended drawNow skips render
    h.flushFrame()
    expect(h.renders()).toBe(0)
    expect(h.realPasses()).toBe(0)
  })
})
