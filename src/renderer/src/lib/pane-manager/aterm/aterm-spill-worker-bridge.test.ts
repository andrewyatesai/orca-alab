/**
 * @vitest-environment happy-dom
 */
import { describe, expect, it } from 'vitest'
import type { AtermDeviceRect } from './aterm-chrome-box'
import { createAtermSpillOverlay } from './aterm-spill-overlay'
import type { SpillPaneGeometry } from './aterm-spill-pane-scratch'
import {
  createAtermSpillWorkerBridge,
  type AtermSpillWorkerChannel
} from './aterm-spill-worker-bridge'
import type { AtermWorkerSpillCommand } from './aterm-worker-spill-protocol'

// The main-side worker seam (stage 4): geometry pushes coalesce to ONE message
// per measure batch, the canvas transfer ships a MONOTONE epoch per generation
// (the worker-respawn re-init contract), and a worker-only population keeps
// the main-thread overlay canvas dormant.

const rect = (x: number, y: number, width: number, height: number): AtermDeviceRect => ({
  x,
  y,
  width,
  height
})

function geom(x: number): SpillPaneGeometry {
  return {
    frameOrigin: { x, y: 0 },
    clipRect: rect(x, 40, 200, 200),
    stripRects: [rect(x, 0, 200, 40)],
    outsideRects: [rect(x, 0, 200, 40)],
    visible: true
  }
}

function makeChannel(): { channel: AtermSpillWorkerChannel; posts: AtermWorkerSpillCommand[] } {
  const posts: AtermWorkerSpillCommand[] = []
  return { channel: { post: (cmd) => posts.push(cmd) }, posts }
}

function makeTransferableCanvas(): HTMLCanvasElement {
  const canvas = document.createElement('canvas')
  if (typeof canvas.transferControlToOffscreen !== 'function') {
    // happy-dom lacks the transfer API: a one-shot stub mirrors the real
    // contract (a second call on the same element throws).
    let transferred = false
    ;(
      canvas as unknown as { transferControlToOffscreen: () => object }
    ).transferControlToOffscreen = () => {
      if (transferred) {
        throw new Error('InvalidStateError: already transferred')
      }
      transferred = true
      return { __offscreen: true }
    }
  }
  return canvas
}

const flushMicrotasks = async (): Promise<void> => {
  await Promise.resolve()
  await Promise.resolve()
}

function makeHarness(): {
  overlay: ReturnType<typeof createAtermSpillOverlay>
  bridge: ReturnType<typeof createAtermSpillWorkerBridge>
} {
  const overlay = createAtermSpillOverlay()
  return { overlay, bridge: createAtermSpillWorkerBridge(overlay) }
}

describe('aterm spill worker bridge', () => {
  it('coalesces N geometry pushes into ONE spillPaneRects message (last wins)', async () => {
    const { overlay, bridge } = makeHarness()
    const { channel, posts } = makeChannel()
    overlay.register('tab:a', { chromePadPx: 13, chromeHeadPx: 34 })
    bridge.bindPane('tab:a', channel)
    overlay.updateGeometry('tab:a', geom(1))
    overlay.updateGeometry('tab:a', geom(2))
    overlay.updateGeometry('tab:a', geom(3))
    expect(posts.filter((cmd) => cmd.type === 'spillPaneRects')).toHaveLength(0)
    await flushMicrotasks()
    const rects = posts.filter((cmd) => cmd.type === 'spillPaneRects')
    expect(rects).toHaveLength(1)
    expect(rects[0]).toMatchObject({ paneKey: 'tab:a', geometry: geom(3) })
    // An identical re-measure posts nothing (change-only contract).
    overlay.updateGeometry('tab:a', geom(3))
    await flushMicrotasks()
    expect(posts.filter((cmd) => cmd.type === 'spillPaneRects')).toHaveLength(1)
  })

  it('ships a fresh canvas generation with a HIGHER epoch after release (worker respawn re-init)', () => {
    const { overlay, bridge } = makeHarness()
    const first = makeChannel()
    overlay.register('tab:a', { chromePadPx: 13, chromeHeadPx: 34 })
    bridge.bindPane('tab:a', first.channel)
    const epoch1 = bridge.getCanvasGeneration()
    const canvasA = makeTransferableCanvas()
    bridge.attachWorkerCanvas(canvasA)
    // Same element re-attached (StrictMode / re-render): no duplicate init.
    bridge.attachWorkerCanvas(canvasA)
    const inits1 = first.posts.filter((cmd) => cmd.type === 'spillCanvasInit')
    expect(inits1).toHaveLength(1)
    expect(inits1[0]).toMatchObject({ epoch: epoch1 })

    // Last worker pane leaves: strips cleared + canvas released.
    overlay.unregister('tab:a')
    expect(first.posts.map((cmd) => cmd.type)).toContain('spillUnregister')
    const releases = first.posts.filter((cmd) => cmd.type === 'spillRelease')
    expect(releases).toHaveLength(1)
    expect(releases[0]).toMatchObject({ epoch: epoch1 })
    expect(bridge.hasWorkerPanes()).toBe(false)

    // Re-bind (fresh worker after a respawn): new generation, new element,
    // strictly higher epoch — the worker's dead-epoch guard depends on it.
    const second = makeChannel()
    overlay.register('tab:a', { chromePadPx: 13, chromeHeadPx: 34 })
    bridge.bindPane('tab:a', second.channel)
    const epoch2 = bridge.getCanvasGeneration()
    expect(epoch2).toBeGreaterThan(epoch1)
    bridge.attachWorkerCanvas(makeTransferableCanvas())
    const inits2 = second.posts.filter((cmd) => cmd.type === 'spillCanvasInit')
    expect(inits2).toHaveLength(1)
    expect(inits2[0]).toMatchObject({ epoch: epoch2 })
  })

  it('forwards overlay box changes to the worker canvas (change-fed only)', () => {
    const { overlay, bridge } = makeHarness()
    const { channel, posts } = makeChannel()
    overlay.setOverlayBox({ widthPx: 640, heightPx: 480 })
    overlay.register('tab:a', { chromePadPx: 13, chromeHeadPx: 34 })
    bridge.bindPane('tab:a', channel)
    bridge.attachWorkerCanvas(makeTransferableCanvas())
    const init = posts.find((cmd) => cmd.type === 'spillCanvasInit')
    expect(init).toMatchObject({ box: { widthPx: 640, heightPx: 480 } })
    overlay.setOverlayBox({ widthPx: 800, heightPx: 600 })
    overlay.setOverlayBox({ widthPx: 800, heightPx: 600 }) // unchanged → silent
    const boxes = posts.filter((cmd) => cmd.type === 'spillOverlayBox')
    expect(boxes).toHaveLength(1)
    expect(boxes[0]).toMatchObject({ box: { widthPx: 800, heightPx: 600 } })
  })

  it('keeps the MAIN overlay canvas dormant while only worker panes are registered', () => {
    const { overlay, bridge } = makeHarness()
    const { channel } = makeChannel()
    const mainCanvas = document.createElement('canvas')
    // happy-dom has no 2d context; the idle/live box logic needs a truthy one.
    ;(mainCanvas as unknown as { getContext: () => object }).getContext = () => ({
      clearRect: () => undefined,
      drawImage: () => undefined
    })
    overlay.attachCanvas(mainCanvas)
    overlay.setOverlayBox({ widthPx: 800, heightPx: 600 })
    overlay.register('tab:a', { chromePadPx: 13, chromeHeadPx: 34 })
    bridge.bindPane('tab:a', channel)
    // The register→delegate transient settles on the queued recomposite.
    return Promise.resolve().then(() => {
      expect(mainCanvas.width).toBe(0)
      expect(mainCanvas.height).toBe(0)
      expect(mainCanvas.style.display).toBe('none')
    })
  })
})
