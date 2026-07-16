import { afterEach, describe, expect, it, vi } from 'vitest'
import type { AtermSharedWorkerPane } from './aterm-shared-render-worker'
import type { AtermDrawerBuildConfig } from './aterm-drawer-config'

vi.mock('./aterm-shared-render-worker', () => ({
  acquireAtermSharedWorkerPane: vi.fn()
}))
vi.mock('./aterm-worker-prewarm', () => ({
  noteRealAtermWorkerPaneAcquired: vi.fn()
}))
vi.mock('./aterm-gpu-auto-policy', () => ({
  decideAtermGpu: vi.fn(() => ({ useGpu: false, reason: 'test' }))
}))

import { loadAtermWorkerEngine } from './aterm-worker-loader'
import { acquireAtermSharedWorkerPane } from './aterm-shared-render-worker'
import { noteRealAtermWorkerPaneAcquired } from './aterm-worker-prewarm'

type FakePane = AtermSharedWorkerPane & {
  post: ReturnType<typeof vi.fn>
  reportBootWedged: ReturnType<typeof vi.fn>
  release: ReturnType<typeof vi.fn>
}

function makeFakePane(): FakePane {
  return {
    paneId: 1,
    post: vi.fn(),
    onEvent: vi.fn(),
    onCrash: vi.fn(),
    isBooted: () => false,
    reportBootWedged: vi.fn(),
    release: vi.fn()
  } as unknown as FakePane
}

function makeConfig(): AtermDrawerBuildConfig {
  return {
    canvas: {
      style: {},
      transferControlToOffscreen: () => ({}) as OffscreenCanvas
    } as unknown as HTMLCanvasElement,
    themeColors: {
      fg: 0xfafafa,
      bg: 0x0a0a0a,
      cursor: 0xfafafa,
      selection: 0x64748b,
      selectionForeground: null,
      selectionInactive: null,
      palette: []
    },
    fontPx: 14,
    lineHeight: 1
  }
}

async function flushMicrotasks(rounds = 8): Promise<void> {
  for (let i = 0; i < rounds; i++) {
    await Promise.resolve()
  }
}

afterEach(() => {
  vi.useRealTimers()
})

describe('loadAtermWorkerEngine boot path', () => {
  it('notes the real pane acquire for the prewarm hold, then handles a boot wedge', async () => {
    vi.useFakeTimers()
    const pane = makeFakePane()
    vi.mocked(acquireAtermSharedWorkerPane).mockResolvedValue(pane)

    const pending = loadAtermWorkerEngine(makeConfig())
    // Swallow the expected boot-timeout rejection (asserted below) so the
    // in-flight promise never surfaces as unhandled while timers advance.
    const settled = pending.catch((err: Error) => err)
    await flushMicrotasks()

    // The prewarm hold releases the moment a REAL pane owns the worker.
    expect(noteRealAtermWorkerPaneAcquired).toHaveBeenCalledTimes(1)
    // The canvas was handed over with the boot init.
    expect(pane.post).toHaveBeenCalledWith(
      expect.objectContaining({ type: 'init', engine: 'cpu' }),
      expect.anything()
    )

    // No 'booted' ack and no first frame: the boot deadline retires the worker
    // and frees this pane's slot so the caller can fall back in-process.
    vi.advanceTimersByTime(4000)
    await flushMicrotasks()
    const err = await settled
    expect(String(err)).toContain('boot timed out')
    expect(pane.reportBootWedged).toHaveBeenCalledTimes(1)
    expect(pane.post).toHaveBeenCalledWith({ type: 'dispose' })
    expect(pane.release).toHaveBeenCalledTimes(1)
  })
})
