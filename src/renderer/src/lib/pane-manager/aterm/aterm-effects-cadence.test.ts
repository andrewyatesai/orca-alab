import { afterEach, describe, expect, it, vi } from 'vitest'
import { createAtermEffectsDrive } from './aterm-effects-drive'
import {
  createWorkerFrameScheduler,
  type WorkerFrameScheduler
} from './aterm-worker-frame-scheduler'
import type { AtermWorkerState } from './aterm-render-worker-protocol'

afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

describe('in-process effects cadence', () => {
  it('uses one exact timer for rain instead of display rAF', () => {
    vi.useFakeTimers()
    const advance = vi.fn()
    const scheduleDraw = vi.fn()
    const drive = createAtermEffectsDrive({
      term: {
        advance_effects: advance,
        is_effects_active: () => true,
        effects_next_deadline_ms: () => 83
      },
      scheduleDraw,
      isDisposed: () => false
    })

    drive.beforeFrame()
    drive.afterFrame()
    expect(scheduleDraw).not.toHaveBeenCalled()
    vi.advanceTimersByTime(82)
    expect(scheduleDraw).not.toHaveBeenCalled()
    vi.advanceTimersByTime(1)
    expect(advance).toHaveBeenLastCalledWith(83)
    expect(scheduleDraw).toHaveBeenCalledTimes(1)
  })

  it('keeps rAF for frame-rate effects and lets real frames preempt rain timers', () => {
    vi.useFakeTimers()
    let deadline: number | undefined
    const scheduleDraw = vi.fn()
    const drive = createAtermEffectsDrive({
      term: {
        advance_effects: vi.fn(),
        is_effects_active: () => true,
        effects_next_deadline_ms: () => deadline
      },
      scheduleDraw,
      isDisposed: () => false
    })

    drive.beforeFrame()
    drive.afterFrame()
    expect(scheduleDraw).toHaveBeenCalledTimes(1)

    scheduleDraw.mockClear()
    deadline = 83
    drive.afterFrame()
    drive.beforeFrame()
    vi.advanceTimersByTime(100)
    expect(scheduleDraw).not.toHaveBeenCalled()
  })

  it('charges a late timer by bounded wall time without rebasing away the gap', () => {
    vi.useFakeTimers()
    const now = vi.spyOn(performance, 'now').mockReturnValue(0)
    const advance = vi.fn()
    const scheduleDraw = vi.fn()
    const drive = createAtermEffectsDrive({
      term: {
        advance_effects: advance,
        is_effects_active: () => true,
        effects_next_deadline_ms: () => 83
      },
      scheduleDraw,
      isDisposed: () => false
    })

    drive.beforeFrame()
    drive.afterFrame()
    now.mockReturnValue(600)
    vi.advanceTimersByTime(83)

    expect(advance).toHaveBeenLastCalledWith(250)
    expect(scheduleDraw).toHaveBeenCalledTimes(1)

    now.mockReturnValue(616)
    drive.beforeFrame()
    expect(advance).toHaveBeenLastCalledWith(16)
  })
})

function workerHarness(
  active: boolean,
  deadline: number | undefined
): {
  scheduler: WorkerFrameScheduler
  render: ReturnType<typeof vi.fn>
  post: ReturnType<typeof vi.fn>
  advance: ReturnType<typeof vi.fn>
  raf: (() => void)[]
} {
  const raf: (() => void)[] = []
  const render = vi.fn()
  const post = vi.fn()
  const advance = vi.fn()
  const state = { cols: 80, rows: 24, cellWidth: 8, cellHeight: 16 } as AtermWorkerState
  const term = {
    render,
    tickEffects: () => active,
    effectsIdleDeadlineMs: () => deadline,
    advanceEffectsBy: advance,
    buildState: () => state,
    dimensions: () => state
  }
  const scheduler = createWorkerFrameScheduler({
    getTerm: () => term,
    post,
    raf: (cb) => raf.push(cb)
  })
  return { scheduler, render, post, advance, raf }
}

function flushRaf(queue: (() => void)[]): void {
  for (const callback of queue.splice(0, queue.length)) {
    callback()
  }
}

describe('worker effects cadence', () => {
  it('renders rain only when its engine deadline matures', () => {
    vi.useFakeTimers()
    const h = workerHarness(true, 83)
    h.scheduler.schedule()
    flushRaf(h.raf)
    expect(h.render).toHaveBeenCalledTimes(1)
    expect(h.raf).toHaveLength(0)

    vi.advanceTimersByTime(82)
    expect(h.raf).toHaveLength(0)
    vi.advanceTimersByTime(1)
    expect(h.raf).toHaveLength(1)
    flushRaf(h.raf)
    expect(h.render).toHaveBeenCalledTimes(2)
  })

  it('holds calm rain to its 12 Hz budget across ten seconds', () => {
    vi.useFakeTimers()
    const h = workerHarness(true, 83)
    h.scheduler.schedule()
    flushRaf(h.raf)

    for (let tick = 0; tick < 120; tick++) {
      vi.advanceTimersByTime(83)
      expect(h.raf).toHaveLength(1)
      flushRaf(h.raf)
    }

    expect(h.render).toHaveBeenCalledTimes(121)
    expect(vi.getTimerCount()).toBe(1)
  })

  it('keeps an rAF continuation for water, glow, and decoration motion', () => {
    const h = workerHarness(true, undefined)
    h.scheduler.schedule()
    flushRaf(h.raf)
    expect(h.render).toHaveBeenCalledTimes(1)
    expect(h.raf).toHaveLength(1)
  })

  it('charges a late worker timer by bounded wall time before scheduling its frame', () => {
    vi.useFakeTimers()
    const now = vi.spyOn(performance, 'now').mockReturnValue(0)
    const h = workerHarness(true, 83)
    h.scheduler.schedule()
    flushRaf(h.raf)

    now.mockReturnValue(600)
    vi.advanceTimersByTime(83)

    expect(h.advance).toHaveBeenLastCalledWith(250)
    expect(h.raf).toHaveLength(1)
  })

  it('makes a queued render obsolete after eager present unless newer state arrives', () => {
    const h = workerHarness(false, undefined)
    h.scheduler.schedule()
    h.scheduler.presentNow()
    expect(h.render).toHaveBeenCalledTimes(1)
    flushRaf(h.raf)
    expect(h.render).toHaveBeenCalledTimes(1)

    h.scheduler.schedule()
    h.scheduler.presentNow()
    h.scheduler.schedule()
    flushRaf(h.raf)
    expect(h.render).toHaveBeenCalledTimes(3)
  })

  it('keeps a genuinely newer eager present after the queued render has landed', () => {
    const h = workerHarness(false, undefined)
    h.scheduler.schedule()
    flushRaf(h.raf)
    expect(h.render).toHaveBeenCalledTimes(1)

    h.scheduler.presentNow()
    expect(h.render).toHaveBeenCalledTimes(2)
  })
})

describe('worker hover STATE-only post', () => {
  it('posts the hover STATE without rendering the engine framebuffer', () => {
    const h = workerHarness(false, undefined)
    h.scheduler.scheduleStatePost()
    flushRaf(h.raf)
    // The hover underline + cursor are main-thread overlays, so the frame posts STATE but the
    // engine framebuffer is byte-identical — render() would be pure waste during a mouse sweep.
    expect(h.post).toHaveBeenCalledTimes(1)
    expect(h.render).not.toHaveBeenCalled()
  })

  it('lets an already-queued draw carry the hover post instead of double-posting', () => {
    const h = workerHarness(false, undefined)
    h.scheduler.schedule() // real content draw queued for this frame (renders + posts)
    h.scheduler.scheduleStatePost() // hover arrives the same frame → defer to that draw
    flushRaf(h.raf)
    expect(h.render).toHaveBeenCalledTimes(1)
    expect(h.post).toHaveBeenCalledTimes(1)
  })

  it('still renders when a real draw follows a render-free hover post', () => {
    const h = workerHarness(false, undefined)
    h.scheduler.scheduleStatePost()
    flushRaf(h.raf)
    expect(h.render).not.toHaveBeenCalled()

    h.scheduler.schedule() // genuine content change → must render, not just post
    flushRaf(h.raf)
    expect(h.render).toHaveBeenCalledTimes(1)
    expect(h.post).toHaveBeenCalledTimes(2)
  })
})
