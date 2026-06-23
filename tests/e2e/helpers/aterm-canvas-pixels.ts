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
