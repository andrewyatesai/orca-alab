/**
 * @vitest-environment happy-dom
 */
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ManagedPaneInternal } from '../pane-manager-types'
import type { AtermPaneController } from './aterm-pane-controller-types'

vi.mock('./aterm-pane-renderer', () => ({
  createAtermPaneController: vi.fn()
}))
vi.mock('@/store', () => ({
  useAppStore: { getState: () => ({ settings: undefined }) }
}))
// The attach edge also flushes rain pulses and binds spill identity (upstream
// #7214-era wiring); both walk live registries, so stub them for unit isolation.
vi.mock('./aterm-rain-pulse-delivery', () => ({
  flushPendingAtermRainPulsesAtControllerAttach: vi.fn()
}))
vi.mock('../pane-manager-registry', () => ({
  getRegisteredTabIdsForController: vi.fn(() => [])
}))

import {
  ATERM_PANE_BUILD_CUE_DELAY_MS,
  createAtermPaneBuildQueue,
  openAtermPane
} from './aterm-pane-open'
import { createAtermPaneController } from './aterm-pane-renderer'

type Deferred = {
  resolve: (controller: AtermPaneController) => void
  reject: (err: Error) => void
}

const mockCreate = vi.mocked(createAtermPaneController)

function deferControllerBuilds(): Deferred[] {
  const deferreds: Deferred[] = []
  // Fresh call history per test: assertions below use absolute call counts.
  mockCreate.mockReset()
  mockCreate.mockImplementation(
    () =>
      new Promise<AtermPaneController>((resolve, reject) => {
        deferreds.push({ resolve, reject })
      })
  )
  return deferreds
}

function makeController(): AtermPaneController {
  return {
    dispose: vi.fn(),
    setDrawSuspended: vi.fn(),
    updateTheme: vi.fn(),
    element: document.createElement('div'),
    textarea: document.createElement('textarea')
  } as unknown as AtermPaneController
}

function makePane(id: number): ManagedPaneInternal {
  return {
    id,
    container: document.createElement('div'),
    xtermContainer: document.createElement('div'),
    disposed: false,
    atermController: null,
    terminal: {
      options: {},
      rows: 24,
      cols: 80,
      buffer: { active: { cursorX: 0, cursorY: 0 } },
      paste: vi.fn(),
      __attachController: vi.fn()
    }
  } as unknown as ManagedPaneInternal
}

async function flushMicrotasks(rounds = 12): Promise<void> {
  for (let i = 0; i < rounds; i++) {
    await Promise.resolve()
  }
}

afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

describe('createAtermPaneBuildQueue', () => {
  it('admits up to the limit and hands freed slots to waiters in FIFO order', async () => {
    const queue = createAtermPaneBuildQueue(2)
    const order: number[] = []
    await queue.admit()
    await queue.admit()
    const third = queue.admit().then(() => order.push(3))
    const fourth = queue.admit().then(() => order.push(4))
    await flushMicrotasks()
    expect(order).toEqual([])
    queue.release()
    await third
    expect(order).toEqual([3])
    queue.release()
    await fourth
    expect(order).toEqual([3, 4])
  })

  it('self-admits a waiter past the limit after the fallback deadline', async () => {
    vi.useFakeTimers()
    const queue = createAtermPaneBuildQueue(1)
    await queue.admit()
    let admitted = false
    const waiter = queue.admit().then(() => {
      admitted = true
    })
    await flushMicrotasks()
    expect(admitted).toBe(false)
    // A wedged build must not dam the queue forever.
    vi.advanceTimersByTime(20_000)
    await waiter
    expect(admitted).toBe(true)
    // Both releases stay consistent (no negative counts / stuck slots).
    queue.release()
    queue.release()
    await queue.admit()
  })
})

describe('openAtermPane', () => {
  it('staggers concurrent pane builds and starts the next when one finishes', async () => {
    const deferreds = deferControllerBuilds()
    const panes = [makePane(1), makePane(2), makePane(3), makePane(4)]
    for (const pane of panes) {
      openAtermPane(pane)
    }
    await flushMicrotasks()
    expect(mockCreate).toHaveBeenCalledTimes(2)

    deferreds[0].resolve(makeController())
    await flushMicrotasks()
    expect(mockCreate).toHaveBeenCalledTimes(3)

    // A failed build releases its slot too.
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => undefined)
    deferreds[1].reject(new Error('boom'))
    await flushMicrotasks()
    expect(mockCreate).toHaveBeenCalledTimes(4)
    expect(errorSpy).toHaveBeenCalled()

    deferreds[2].resolve(makeController())
    deferreds[3].resolve(makeController())
    await flushMicrotasks()
  })

  it('skips a queued pane disposed before its build slot frees', async () => {
    const deferreds = deferControllerBuilds()
    const first = makePane(1)
    const second = makePane(2)
    const queued = makePane(3)
    openAtermPane(first)
    openAtermPane(second)
    openAtermPane(queued)
    await flushMicrotasks()
    expect(mockCreate).toHaveBeenCalledTimes(2)

    queued.disposed = true
    deferreds[0].resolve(makeController())
    await flushMicrotasks()
    // The freed slot must not build the disposed pane...
    expect(mockCreate).toHaveBeenCalledTimes(2)

    // ...and must be available again for the next open.
    const fourth = makePane(4)
    openAtermPane(fourth)
    await flushMicrotasks()
    expect(mockCreate).toHaveBeenCalledTimes(3)

    deferreds[1].resolve(makeController())
    deferreds[2].resolve(makeController())
    await flushMicrotasks()
  })

  it('shows the busy cursor only past the deferral window and clears it on attach', async () => {
    vi.useFakeTimers()
    const deferreds = deferControllerBuilds()
    const pane = makePane(1)
    openAtermPane(pane)
    await flushMicrotasks()
    expect(pane.container.style.cursor).toBe('')

    vi.advanceTimersByTime(ATERM_PANE_BUILD_CUE_DELAY_MS)
    expect(pane.container.style.cursor).toBe('progress')

    deferreds[0].resolve(makeController())
    await flushMicrotasks()
    expect(pane.container.style.cursor).toBe('')
  })

  it('never shows the cue for a warm (fast) open', async () => {
    const deferreds = deferControllerBuilds()
    const pane = makePane(1)
    openAtermPane(pane)
    await flushMicrotasks()
    deferreds[0].resolve(makeController())
    await flushMicrotasks()
    expect(pane.container.style.cursor).toBe('')
  })

  it('attaches the controller and keeps the container background synced on re-theme', async () => {
    const deferreds = deferControllerBuilds()
    const pane = makePane(1)
    openAtermPane(pane)
    await flushMicrotasks()
    const controller = makeController()
    const engineUpdateTheme = controller.updateTheme
    deferreds[0].resolve(controller)
    await flushMicrotasks()

    expect(pane.atermController).toBe(controller)
    expect(pane.terminal.__attachController).toHaveBeenCalledWith(controller, {
      element: controller.element,
      textarea: controller.textarea
    })

    // Live re-theme flows through the controller — the container's never-blank
    // paint must follow so the DOM behind the canvas never goes stale.
    const colors = { bg: 0x123456 } as never
    pane.atermController!.updateTheme(colors)
    expect(pane.container.style.background).toBe('#123456')
    expect(engineUpdateTheme).toHaveBeenCalledWith(colors)
  })

  it('starts draw-suspended panes paused', async () => {
    const deferreds = deferControllerBuilds()
    const pane = makePane(1)
    pane.startRenderingSuspended = true
    openAtermPane(pane)
    await flushMicrotasks()
    const controller = makeController()
    deferreds[0].resolve(controller)
    await flushMicrotasks()
    expect(controller.setDrawSuspended).toHaveBeenCalledWith(true)
  })

  it('drops the controller when the pane was disposed during the build', async () => {
    const deferreds = deferControllerBuilds()
    const pane = makePane(1)
    openAtermPane(pane)
    await flushMicrotasks()
    pane.disposed = true
    const controller = makeController()
    deferreds[0].resolve(controller)
    await flushMicrotasks()
    expect(controller.dispose).toHaveBeenCalledTimes(1)
    expect(pane.atermController).toBeNull()
  })
})
