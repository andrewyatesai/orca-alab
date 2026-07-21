import { describe, expect, it, vi } from 'vitest'
import { beginSplitResizeDrag } from './tab-group-split-resize-drag'

type StyledEl = { style: { flex: string } }

function createContainer(rect: { left: number; top: number; width: number; height: number }) {
  return {
    getBoundingClientRect: () => ({
      left: rect.left,
      top: rect.top,
      right: rect.left + rect.width,
      bottom: rect.top + rect.height,
      width: rect.width,
      height: rect.height
    })
  } as unknown as HTMLElement
}

function createHandleHarness({
  rect = { left: 0, top: 0, width: 1000, height: 500 },
  prevFlex = '0.5 1 0%',
  nextFlex = '0.5 1 0%',
  prevEl,
  nextEl
}: {
  rect?: { left: number; top: number; width: number; height: number }
  prevFlex?: string
  nextFlex?: string
  prevEl?: StyledEl | null
  nextEl?: StyledEl | null
} = {}) {
  const listeners = new Map<string, EventListener>()
  const captured = new Set<number>()
  const resolvedPrev: StyledEl | null =
    prevEl === undefined ? { style: { flex: prevFlex } } : prevEl
  const resolvedNext: StyledEl | null =
    nextEl === undefined ? { style: { flex: nextFlex } } : nextEl
  const releasePointerCapture = vi.fn((id: number) => {
    captured.delete(id)
  })
  const handle = {
    parentElement: createContainer(rect),
    previousElementSibling: resolvedPrev,
    nextElementSibling: resolvedNext,
    setPointerCapture: (id: number) => captured.add(id),
    hasPointerCapture: (id: number) => captured.has(id),
    releasePointerCapture,
    addEventListener: (event: string, listener: EventListener) => {
      listeners.set(event, listener)
    },
    removeEventListener: (event: string, listener: EventListener) => {
      if (listeners.get(event) === listener) {
        listeners.delete(event)
      }
    }
  } as unknown as HTMLElement

  return { handle, listeners, captured, releasePointerCapture, prevEl: resolvedPrev, nextEl: resolvedNext }
}

function createFrameControls() {
  const queued: FrameRequestCallback[] = []
  const requestFrame = vi.fn((callback: FrameRequestCallback) => {
    queued.push(callback)
    return queued.length
  })
  const cancelFrame = vi.fn()
  const runFrames = (): void => {
    const pending = queued.splice(0)
    for (const callback of pending) {
      callback(16)
    }
  }
  return { requestFrame, cancelFrame, runFrames, queued }
}

function pointer(args: {
  pointerId?: number
  clientX?: number
  clientY?: number
  isPrimary?: boolean
}): PointerEvent {
  return { pointerId: 1, clientX: 0, clientY: 0, ...args } as unknown as PointerEvent
}

describe('beginSplitResizeDrag', () => {
  it('returns null when the divider has no resizable siblings', () => {
    const onRatioChange = vi.fn()
    const { handle } = createHandleHarness({ prevEl: null })
    const cleanup = beginSplitResizeDrag({
      handle,
      pointerId: 1,
      isHorizontal: true,
      minRatio: 0.15,
      maxRatio: 0.85,
      onRatioChange
    })
    expect(cleanup).toBeNull()
    expect(onRatioChange).not.toHaveBeenCalled()
  })

  it('coalesces raw pointermoves into one live flex write per frame and never touches the store mid-drag', () => {
    const onRatioChange = vi.fn()
    const { handle, listeners, prevEl, nextEl } = createHandleHarness()
    const frames = createFrameControls()

    beginSplitResizeDrag({
      handle,
      pointerId: 1,
      isHorizontal: true,
      minRatio: 0.15,
      maxRatio: 0.85,
      onRatioChange,
      requestFrame: frames.requestFrame,
      cancelFrame: frames.cancelFrame
    })

    const move = listeners.get('pointermove')
    move?.(pointer({ clientX: 300 }))
    move?.(pointer({ clientX: 500 }))
    move?.(pointer({ clientX: 600 }))

    // Three raw moves, a single scheduled frame, and zero store writes so far.
    expect(frames.requestFrame).toHaveBeenCalledTimes(1)
    expect(onRatioChange).not.toHaveBeenCalled()
    expect(prevEl?.style.flex).toBe('0.5 1 0%')

    frames.runFrames()

    // Only the final ratio is painted to the DOM.
    expect(prevEl?.style.flex).toBe('0.6 1 0%')
    expect(nextEl?.style.flex).toBe('0.4 1 0%')
    expect(onRatioChange).not.toHaveBeenCalled()
  })

  it('commits the final ratio to the store exactly once on pointerup', () => {
    const onRatioChange = vi.fn()
    const onEnd = vi.fn()
    const { handle, listeners, prevEl, nextEl, releasePointerCapture } = createHandleHarness()
    const frames = createFrameControls()

    beginSplitResizeDrag({
      handle,
      pointerId: 7,
      isHorizontal: true,
      minRatio: 0.15,
      maxRatio: 0.85,
      onRatioChange,
      onEnd,
      requestFrame: frames.requestFrame,
      cancelFrame: frames.cancelFrame
    })

    const move = listeners.get('pointermove')
    move?.(pointer({ pointerId: 7, clientX: 300 }))
    frames.runFrames()
    // A move after the last painted frame leaves a pending frame that pointerup must flush.
    move?.(pointer({ pointerId: 7, clientX: 600 }))

    listeners.get('pointerup')?.(pointer({ pointerId: 7, clientX: 600 }))

    expect(onRatioChange).toHaveBeenCalledTimes(1)
    expect(onRatioChange).toHaveBeenCalledWith(0.6)
    // pointerup flushed the pending frame, so the DOM shows the final ratio.
    expect(prevEl?.style.flex).toBe('0.6 1 0%')
    expect(nextEl?.style.flex).toBe('0.4 1 0%')
    expect(releasePointerCapture).toHaveBeenCalledWith(7)
    expect(onEnd).toHaveBeenCalledTimes(1)
    // Listeners are torn down so a later stray event cannot double-commit.
    expect(listeners.has('pointermove')).toBe(false)
    expect(listeners.has('pointerup')).toBe(false)
  })

  it('continues the resize when motion arrives from a different primary pointer (WSLg pen relay)', () => {
    const onRatioChange = vi.fn()
    const { handle, listeners, prevEl, nextEl } = createHandleHarness()
    const frames = createFrameControls()

    beginSplitResizeDrag({
      handle,
      pointerId: 1,
      isHorizontal: true,
      minRatio: 0.15,
      maxRatio: 0.85,
      onRatioChange,
      requestFrame: frames.requestFrame,
      cancelFrame: frames.cancelFrame
    })

    // WSLg's RDP relay presses as `mouse` (id 1) but streams motion as a `pen`
    // pointer with a different pointerId; both are the primary pointer.
    listeners.get('pointermove')?.(pointer({ pointerId: 19, isPrimary: true, clientX: 300 }))
    frames.runFrames()

    expect(prevEl?.style.flex).toBe('0.3 1 0%')
    expect(nextEl?.style.flex).toBe('0.7 1 0%')

    listeners.get('pointerup')?.(pointer({ pointerId: 1, clientX: 300 }))
    expect(onRatioChange).toHaveBeenCalledWith(0.3)
  })

  it('ignores motion from a non-primary secondary pointer during the drag', () => {
    const onRatioChange = vi.fn()
    const { handle, listeners, prevEl } = createHandleHarness()
    const frames = createFrameControls()

    beginSplitResizeDrag({
      handle,
      pointerId: 1,
      isHorizontal: true,
      minRatio: 0.15,
      maxRatio: 0.85,
      onRatioChange,
      requestFrame: frames.requestFrame,
      cancelFrame: frames.cancelFrame
    })

    listeners.get('pointermove')?.(pointer({ pointerId: 5, isPrimary: false, clientX: 300 }))
    frames.runFrames()

    expect(prevEl?.style.flex).toBe('0.5 1 0%')
    listeners.get('pointerup')?.(pointer({ pointerId: 1 }))
    expect(onRatioChange).not.toHaveBeenCalled()
  })

  it('clamps the committed ratio to the min/max bounds', () => {
    const onRatioChange = vi.fn()
    const { handle, listeners } = createHandleHarness()
    const frames = createFrameControls()

    beginSplitResizeDrag({
      handle,
      pointerId: 1,
      isHorizontal: true,
      minRatio: 0.15,
      maxRatio: 0.85,
      onRatioChange,
      requestFrame: frames.requestFrame,
      cancelFrame: frames.cancelFrame
    })

    // clientX 980 / width 1000 = 0.98 -> clamped to 0.85.
    listeners.get('pointermove')?.(pointer({ clientX: 980 }))
    listeners.get('pointerup')?.(pointer({ clientX: 980 }))

    expect(onRatioChange).toHaveBeenCalledWith(0.85)
  })

  it('resolves the ratio along the vertical axis for column splits', () => {
    const onRatioChange = vi.fn()
    const { handle, listeners } = createHandleHarness()
    const frames = createFrameControls()

    beginSplitResizeDrag({
      handle,
      pointerId: 1,
      isHorizontal: false,
      minRatio: 0.15,
      maxRatio: 0.85,
      onRatioChange,
      requestFrame: frames.requestFrame,
      cancelFrame: frames.cancelFrame
    })

    // clientY 200 / height 500 = 0.4.
    listeners.get('pointermove')?.(pointer({ clientX: 999, clientY: 200 }))
    listeners.get('pointerup')?.(pointer({ clientX: 999, clientY: 200 }))

    expect(onRatioChange).toHaveBeenCalledWith(0.4)
  })

  it('does not commit when the pointer never moved', () => {
    const onRatioChange = vi.fn()
    const onEnd = vi.fn()
    const { handle, listeners } = createHandleHarness()
    const frames = createFrameControls()

    beginSplitResizeDrag({
      handle,
      pointerId: 1,
      isHorizontal: true,
      minRatio: 0.15,
      maxRatio: 0.85,
      onRatioChange,
      onEnd,
      requestFrame: frames.requestFrame,
      cancelFrame: frames.cancelFrame
    })

    listeners.get('pointerup')?.(pointer({ clientX: 0 }))

    expect(onRatioChange).not.toHaveBeenCalled()
    expect(onEnd).toHaveBeenCalledTimes(1)
  })

  it('reverts the live flex and skips the store on pointercancel', () => {
    const onRatioChange = vi.fn()
    const onEnd = vi.fn()
    const { handle, listeners, prevEl, nextEl } = createHandleHarness({
      prevFlex: '0.5 1 0%',
      nextFlex: '0.5 1 0%'
    })
    const frames = createFrameControls()

    beginSplitResizeDrag({
      handle,
      pointerId: 1,
      isHorizontal: true,
      minRatio: 0.15,
      maxRatio: 0.85,
      onRatioChange,
      onEnd,
      requestFrame: frames.requestFrame,
      cancelFrame: frames.cancelFrame
    })

    listeners.get('pointermove')?.(pointer({ clientX: 700 }))
    frames.runFrames()
    expect(prevEl?.style.flex).toBe('0.7 1 0%')

    listeners.get('pointercancel')?.(pointer({}))

    // Flex is restored to its pre-drag basis and the store is left untouched.
    expect(prevEl?.style.flex).toBe('0.5 1 0%')
    expect(nextEl?.style.flex).toBe('0.5 1 0%')
    expect(onRatioChange).not.toHaveBeenCalled()
    expect(frames.cancelFrame).not.toHaveBeenCalled()
    expect(onEnd).toHaveBeenCalledTimes(1)
  })

  it('tears down silently without an onEnd callback on unmount', () => {
    const onRatioChange = vi.fn()
    const onEnd = vi.fn()
    const { handle, listeners } = createHandleHarness()
    const frames = createFrameControls()

    const cleanup = beginSplitResizeDrag({
      handle,
      pointerId: 1,
      isHorizontal: true,
      minRatio: 0.15,
      maxRatio: 0.85,
      onRatioChange,
      onEnd,
      requestFrame: frames.requestFrame,
      cancelFrame: frames.cancelFrame
    })

    cleanup?.({ commit: false, silent: true })

    expect(onEnd).not.toHaveBeenCalled()
    expect(onRatioChange).not.toHaveBeenCalled()
    expect(listeners.has('pointermove')).toBe(false)
  })
})
