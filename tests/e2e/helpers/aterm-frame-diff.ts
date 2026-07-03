// Frame-diff helpers for the aterm animation/effects e2e specs: snapshot a
// canvas (or a cell band) by key, then count changed pixels on later frames —
// how the specs prove "it animates" and "it settles". Split from
// aterm-canvas-pixels.ts to keep both under the line budget.

import type { Page } from '@playwright/test'

// In-page canvas resolver shared by the snapshot/diff helpers: by ptyId when
// given (multi-tab specs — the FIRST canvas in DOM order can belong to a
// backgrounded pane), else the document-first canvas (single-pane specs).
const RESOLVE_CANVAS_SRC = `(ptyId) => {
  if (ptyId) {
    const managers = window.__paneManagers
    for (const mgr of managers?.values() ?? []) {
      for (const pane of mgr.getPanes?.() ?? []) {
        if (pane?.container?.dataset?.ptyId === ptyId) {
          return pane.container.querySelector('[data-testid="aterm-canvas"]')
        }
      }
    }
    return null
  }
  return document.querySelector('[data-testid="aterm-canvas"]')
}`
/** Snapshot the aterm canvas RGBA into a PAGE-SIDE global under `key`, WITHOUT
 *  transferring the buffer over IPC. Pair with `countAtermChangedPixelsSince` so the
 *  whole-canvas diff runs in-page and only a small count crosses the boundary — a
 *  full-buffer `readAtermRgba` round-trip is multi-second on large/Retina canvases, so
 *  polling it repeatedly (typing→repaint waits) blows the test timeout. Works on the
 *  webgl2 swapchain and the 2d canvas alike. Returns false when the canvas isn't ready.
 *  Pass `ptyId` to scope to that pane's canvas (document-first otherwise). */
export async function snapshotAtermCanvas(
  page: Page,
  key: string,
  ptyId?: string
): Promise<boolean> {
  return page.evaluate(
    ({ k, ptyId, findSrc }) => {
      // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
      const find = new Function(`return (${findSrc})`)() as (
        id: string | undefined
      ) => HTMLCanvasElement | null
      const c = find(ptyId)
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
    },
    { k: key, ptyId, findSrc: RESOLVE_CANVAS_SRC }
  )
}

/** Snapshot ONLY a horizontal device-pixel BAND of the aterm canvas (rows yTop..
 *  yTop+h) into a page-side global — for "did pixels near the cursor/word row
 *  change" assertions without diffing the whole buffer. Same in-page pattern as
 *  snapshotAtermCanvas; pair with countAtermChangedPixelsInBand. */
export async function snapshotAtermCanvasBand(
  page: Page,
  key: string,
  band: { y: number; h: number },
  ptyId?: string
): Promise<boolean> {
  return page.evaluate(
    ({ k, band, ptyId, findSrc }) => {
      // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
      const find = new Function(`return (${findSrc})`)() as (
        id: string | undefined
      ) => HTMLCanvasElement | null
      const c = find(ptyId)
      if (!c || !c.width || !c.height) {
        return false
      }
      const h = Math.max(1, Math.min(band.h, c.height))
      const yTop = Math.max(0, Math.min(band.y, c.height - h))
      let data: Uint8Array
      const gl = c.getContext('webgl2')
      if (gl) {
        data = new Uint8Array(c.width * h * 4)
        // WebGL origin is bottom-left; flip the row band to top-left coords.
        gl.readPixels(0, c.height - yTop - h, c.width, h, gl.RGBA, gl.UNSIGNED_BYTE, data)
      } else {
        const ctx = c.getContext('2d')
        if (!ctx) {
          return false
        }
        data = new Uint8Array(ctx.getImageData(0, yTop, c.width, h).data.buffer.slice(0))
      }
      const w = window as unknown as {
        __atermBandSnap?: Record<string, { y: number; h: number; data: Uint8Array }>
      }
      w.__atermBandSnap = w.__atermBandSnap ?? {}
      w.__atermBandSnap[k] = { y: yTop, h, data }
      return true
    },
    { k: key, band, ptyId, findSrc: RESOLVE_CANVAS_SRC }
  )
}

/** Count RGB-differing pixels in the band snapshotted under `key` vs the canvas
 *  NOW (in-page; only the count crosses IPC). Size mismatch = fully changed. */
export async function countAtermChangedPixelsInBand(
  page: Page,
  key: string,
  ptyId?: string
): Promise<number> {
  return page.evaluate(
    ({ k, ptyId, findSrc }) => {
      // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
      const find = new Function(`return (${findSrc})`)() as (
        id: string | undefined
      ) => HTMLCanvasElement | null
      const c = find(ptyId)
      const snap = (
        window as unknown as {
          __atermBandSnap?: Record<string, { y: number; h: number; data: Uint8Array }>
        }
      ).__atermBandSnap?.[k]
      if (!c || !c.width || !c.height || !snap) {
        return 0
      }
      let after: Uint8Array
      const gl = c.getContext('webgl2')
      if (gl) {
        after = new Uint8Array(c.width * snap.h * 4)
        gl.readPixels(
          0,
          c.height - snap.y - snap.h,
          c.width,
          snap.h,
          gl.RGBA,
          gl.UNSIGNED_BYTE,
          after
        )
      } else {
        const ctx = c.getContext('2d')
        if (!ctx) {
          return 0
        }
        after = new Uint8Array(ctx.getImageData(0, snap.y, c.width, snap.h).data.buffer)
      }
      if (snap.data.length !== after.length) {
        return after.length / 4
      }
      let changed = 0
      for (let i = 0; i < after.length; i += 4) {
        if (
          after[i] !== snap.data[i] ||
          after[i + 1] !== snap.data[i + 1] ||
          after[i + 2] !== snap.data[i + 2]
        ) {
          changed++
        }
      }
      return changed
    },
    { k: key, ptyId, findSrc: RESOLVE_CANVAS_SRC }
  )
}

/** Count RGB-differing pixels between the aterm canvas NOW and the snapshot stored
 *  under `key` (see `snapshotAtermCanvas`), computed IN-PAGE — only the count crosses
 *  IPC. A size mismatch (canvas resized) counts as fully changed. */
export async function countAtermChangedPixelsSince(
  page: Page,
  key: string,
  ptyId?: string
): Promise<number> {
  return page.evaluate(
    ({ k, ptyId, findSrc }) => {
      // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
      const find = new Function(`return (${findSrc})`)() as (
        id: string | undefined
      ) => HTMLCanvasElement | null
      const c = find(ptyId)
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
    },
    { k: key, ptyId, findSrc: RESOLVE_CANVAS_SRC }
  )
}
