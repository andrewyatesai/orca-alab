import type { Page } from '@playwright/test'

/** The aterm grid canvas can be 2d-owned (CPU draw path) OR webgl2-owned (GPU
 *  draw path — now the default on capable hardware). These helpers read pixels
 *  from EITHER, so a spec asserts on "what the renderer painted" without caring
 *  which path won.
 *
 *  WebGL's framebuffer origin is bottom-left (Y flipped vs the 2d canvas, whose
 *  origin is top-left). `readAtermPixel` flips Y so callers sample by top-left
 *  coords uniformly; `countAtermNonBgPixels` is flip-agnostic (whole-buffer scan,
 *  treating the buffer's first pixel as bg). */

type RgbaBuffer = { w: number; h: number; data: number[] }

/** The aterm canvas's full RGBA buffer (raw, NOT row-flipped), or null when the
 *  canvas isn't ready. Reads via gl.readPixels on the GPU swapchain or
 *  getImageData on the CPU 2d canvas. */
export async function readAtermRgba(page: Page): Promise<RgbaBuffer | null> {
  return page.evaluate(() => {
    const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement | null
    if (!c || !c.width || !c.height) {
      return null
    }
    const w = c.width
    const h = c.height
    const gl = c.getContext('webgl2')
    if (gl) {
      const px = new Uint8Array(w * h * 4)
      gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, px)
      return { w, h, data: Array.from(px) }
    }
    const ctx = c.getContext('2d')
    if (!ctx) {
      return null
    }
    return { w, h, data: Array.from(ctx.getImageData(0, 0, w, h).data) }
  })
}

/** Count pixels whose RGB differs from the buffer's first pixel (treated as bg).
 *  Works on the CPU 2d canvas and the GPU webgl2 swapchain alike; the row order
 *  doesn't matter for a whole-buffer differing-pixel count. */
export async function countAtermNonBgPixels(page: Page): Promise<number> {
  const read = await readAtermRgba(page)
  if (!read) {
    return 0
  }
  const d = read.data
  const bg = [d[0], d[1], d[2]]
  let n = 0
  for (let i = 0; i < d.length; i += 4) {
    if (d[i] !== bg[0] || d[i + 1] !== bg[1] || d[i + 2] !== bg[2]) {
      n++
    }
  }
  return n
}

/** RGB of a single pixel at (x, y) in TOP-LEFT coordinates (both paths), or null.
 *  GPU reads flip Y (WebGL origin is bottom-left) so the coordinate space matches
 *  the 2d canvas. */
export async function readAtermPixel(
  page: Page,
  x: number,
  y: number
): Promise<[number, number, number] | null> {
  return page.evaluate(
    ([px, py]) => {
      const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement | null
      if (!c || !c.width || !c.height) {
        return null
      }
      const h = c.height
      const gl = c.getContext('webgl2')
      if (gl) {
        const buf = new Uint8Array(4)
        gl.readPixels(px, h - 1 - py, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, buf)
        return [buf[0], buf[1], buf[2]] as [number, number, number]
      }
      const ctx = c.getContext('2d')
      if (!ctx) {
        return null
      }
      const d = ctx.getImageData(px, py, 1, 1).data
      return [d[0], d[1], d[2]] as [number, number, number]
    },
    [x, y] as const
  )
}

// The pane that drives the test is the one bound to `ptyId` (where output is
// written + echoed back). With more than one terminal tab a canvas-by-ptyId scope
// is unambiguous, unlike a DOM-first-match or getActivePane() (the managers can
// disagree on which pane is "active" across tabs).
const PANE_CANVAS_BY_PTY = `(ptyId) => {
  const managers = window.__paneManagers
  for (const mgr of managers?.values() ?? []) {
    for (const pane of mgr.getPanes?.() ?? []) {
      if (pane?.container?.dataset?.ptyId === ptyId) {
        return pane.container.querySelector('[data-testid="aterm-canvas"]')
      }
    }
  }
  return null
}`

/** A small device-pixel REGION of the canvas of the pane bound to `ptyId`, as a
 *  flat RGBA array, or null. Reading a small rect (vs the whole multi-megapixel
 *  buffer) keeps the CDP payload tiny — a full-canvas readback can be ~17 MB and
 *  fail to serialize. Top-left origin on both paths (GPU reads flip Y). */
export async function readAtermRegionByPtyId(
  page: Page,
  ptyId: string,
  rect: { x: number; y: number; w: number; h: number }
): Promise<number[] | null> {
  return page.evaluate(
    ({ ptyId, rect, findSrc }) => {
      // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
      const find = new Function(`return (${findSrc})`)() as (id: string) => HTMLCanvasElement | null
      const c = find(ptyId)
      if (!c || !c.width || !c.height) {
        return null
      }
      const x = Math.max(0, Math.min(rect.x, c.width - rect.w))
      const w = Math.max(1, Math.min(rect.w, c.width))
      const h = Math.max(1, Math.min(rect.h, c.height))
      const yTop = Math.max(0, Math.min(rect.y, c.height - h))
      const gl = c.getContext('webgl2')
      if (gl) {
        const px = new Uint8Array(w * h * 4)
        // WebGL origin is bottom-left; flip the row band to top-left coords.
        gl.readPixels(x, c.height - yTop - h, w, h, gl.RGBA, gl.UNSIGNED_BYTE, px)
        return Array.from(px)
      }
      const ctx = c.getContext('2d')
      if (!ctx) {
        return null
      }
      return Array.from(ctx.getImageData(x, yTop, w, h).data)
    },
    { ptyId, rect, findSrc: PANE_CANVAS_BY_PTY }
  )
}

/** WebGL2/2d ownership of the canvas of the pane bound to `ptyId`. */
export async function atermCanvasContextInfoByPtyId(
  page: Page,
  ptyId: string
): Promise<{ gl: boolean; twoD: boolean } | null> {
  return page.evaluate(
    ({ ptyId, findSrc }) => {
      // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
      const find = new Function(`return (${findSrc})`)() as (id: string) => HTMLCanvasElement | null
      const c = find(ptyId)
      if (!c) {
        return null
      }
      return { gl: Boolean(c.getContext('webgl2')), twoD: Boolean(c.getContext('2d')) }
    },
    { ptyId, findSrc: PANE_CANVAS_BY_PTY }
  )
}

/** Force a real WebGL2 context loss on the canvas of the pane bound to `ptyId`
 *  (the same 'webglcontextlost' event the GPU drawer listens for). Returns true if
 *  the WEBGL_lose_context extension was available to force it. */
export async function forceAtermContextLossByPtyId(page: Page, ptyId: string): Promise<boolean> {
  return page.evaluate(
    ({ ptyId, findSrc }) => {
      // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
      const find = new Function(`return (${findSrc})`)() as (id: string) => HTMLCanvasElement | null
      const ext = find(ptyId)?.getContext('webgl2')?.getExtension('WEBGL_lose_context')
      if (ext) {
        ext.loseContext()
        return true
      }
      return false
    },
    { ptyId, findSrc: PANE_CANVAS_BY_PTY }
  )
}

/** True when the aterm canvas is ready to read from (has a 2d OR webgl2 context
 *  AND non-zero dimensions). */
export async function atermCanvasReady(page: Page): Promise<boolean> {
  return page.evaluate(() => {
    const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement | null
    if (!c || !c.width || !c.height) {
      return false
    }
    return Boolean(c.getContext('webgl2') || c.getContext('2d'))
  })
}

/** Snapshot the aterm canvas RGBA into a PAGE-SIDE global under `key`, WITHOUT
 *  transferring the buffer over IPC. Pair with `countAtermChangedPixelsSince` so the
 *  whole-canvas diff runs in-page and only a small count crosses the boundary — a
 *  full-buffer `readAtermRgba` round-trip is multi-second on large/Retina canvases, so
 *  polling it repeatedly (typing→repaint waits) blows the test timeout. Works on the
 *  webgl2 swapchain and the 2d canvas alike. Returns false when the canvas isn't ready. */
export async function snapshotAtermCanvas(page: Page, key: string): Promise<boolean> {
  return page.evaluate((k) => {
    const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement | null
    if (!c || !c.width || !c.height) {
      return false
    }
    const gl = c.getContext('webgl2')
    let data: Uint8Array
    if (gl) {
      data = new Uint8Array(c.width * c.height * 4)
      gl.readPixels(0, 0, c.width, c.height, gl.RGBA, gl.UNSIGNED_BYTE, data)
    } else {
      const ctx = c.getContext('2d')
      if (!ctx) {
        return false
      }
      data = new Uint8Array(ctx.getImageData(0, 0, c.width, c.height).data.buffer.slice(0))
    }
    const w = window as unknown as { __atermSnap?: Record<string, Uint8Array> }
    w.__atermSnap = w.__atermSnap ?? {}
    w.__atermSnap[k] = data
    return true
  }, key)
}

/** Count RGB-differing pixels between the aterm canvas NOW and the snapshot stored
 *  under `key` (see `snapshotAtermCanvas`), computed IN-PAGE — only the count crosses
 *  IPC. A size mismatch (canvas resized) counts as fully changed. */
export async function countAtermChangedPixelsSince(page: Page, key: string): Promise<number> {
  return page.evaluate((k) => {
    const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement | null
    if (!c || !c.width || !c.height) {
      return 0
    }
    const gl = c.getContext('webgl2')
    let after: Uint8Array
    if (gl) {
      after = new Uint8Array(c.width * c.height * 4)
      gl.readPixels(0, 0, c.width, c.height, gl.RGBA, gl.UNSIGNED_BYTE, after)
    } else {
      const ctx = c.getContext('2d')
      if (!ctx) {
        return 0
      }
      after = new Uint8Array(ctx.getImageData(0, 0, c.width, c.height).data.buffer)
    }
    const before = (window as unknown as { __atermSnap?: Record<string, Uint8Array> })
      .__atermSnap?.[k]
    if (!before || before.length !== after.length) {
      return after.length / 4
    }
    let changed = 0
    for (let i = 0; i < after.length; i += 4) {
      if (
        after[i] !== before[i] ||
        after[i + 1] !== before[i + 1] ||
        after[i + 2] !== before[i + 2]
      ) {
        changed++
      }
    }
    return changed
  }, key)
}
