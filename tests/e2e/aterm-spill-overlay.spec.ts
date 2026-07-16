import { test, expect } from './helpers/orca-app'
import type { Page } from '@stablyai/playwright-test'
import {
  closeActiveTerminalPane,
  splitActiveTerminalPane,
  waitForActivePanePtyId
} from './helpers/terminal'
import { waitForAtermControllerByPtyId } from './helpers/aterm-controller'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'
import { countAtermChangedPixelsSince, snapshotAtermCanvas } from './helpers/aterm-frame-diff'

// PROVES the cross-pane effects spill overlay (stage 3, IN-PROCESS render path)
// end-to-end on the real app:
//  (a) enabling the fire cursor glow and typing paints NONZERO overlay pixels in
//      the pane's head-band region OUTSIDE its own clip (the window-space canvas
//      above the pane's top edge — over the split gap and the neighbor pane);
//  (b) SEAM CONTINUITY: during an active burn, a band straddling the pane's top
//      clip line has no fully-transparent overlay row above pixels that are lit
//      on the pane side (approximate continuity — fire flickers per frame, so
//      the check is lit-presence within a widened column window, not byte
//      equality);
//  (c) disabling the glow unregisters every pane and the overlay layer unmounts
//      (the canvas leaves the DOM — cleared by construction);
//  (d) input passthrough: clicking through the lit spill region focuses the
//      pane UNDER the pixels (the overlay is pointer-events: none).
//
// The first describe is the IN-PROCESS slice: it forces __atermWorkerRender =
// false itself and the CPU drawer for deterministic 2d pixel readback, and is
// SKIPPED under ORCA_E2E_ATERM_WORKER=1 (the worker gate must stay a worker
// gate; this path already runs in the default suite). The second describe is
// the WORKER-path slice (stage 4): compositing runs in the shared render
// worker on a transferred overlay canvas, so pixel truth is read by drawImage
// of the PLACEHOLDER canvases (a transferred canvas stays a spec-valid
// drawImage source showing the last committed worker frame) — plus the
// worker-only lifecycle: pane close mid-burn clears its strips, and a worker
// restart re-establishes spill on a fresh canvas generation (epoch bump).

// Overlay pixels are straight-alpha on transparent black; anything above noise
// counts as painted. Pane-side "lit" = differs from the seeded theme background
// (the readback band is chrome-band rows, which carry no glyphs).
const OVERLAY_ALPHA_MIN = 8
const PANE_BG_DIFF_MIN = 24
// Sampled band height above the clip line for the painted-pixels assertion.
const OVERLAY_BAND_PX = 16
const QUIET_WINDOW_MS = 350

type PaneBox = { ptyId: string; top: number; left: number; width: number; height: number }

type SpillSeamProbe = {
  /** Painted overlay pixels in the band above the pane's top clip line. */
  overlayLit: number
  /** Pane-canvas columns lit (vs theme bg) just BELOW the clip line. */
  paneLitCols: number
  /** Rows 1..3 above the clip line with NO overlay pixel inside the lit
   *  column window (0 while the seam is continuous); -1 = pane side not lit. */
  gapRows: number
}

// Type through the pane's helper textarea — the same input path a real
// keystroke takes (input event → inputSink → PTY → echo → engine); scoped by
// ptyId because the document-first textarea belongs to the bootstrap pane.
async function typeIntoPane(page: Page, ptyId: string, text: string): Promise<void> {
  await page.evaluate(
    ({ ptyId, text }) => {
      const managers = (
        window as unknown as {
          __paneManagers?: Map<string, { getPanes?: () => { container?: HTMLElement | null }[] }>
        }
      ).__paneManagers
      for (const mgr of managers?.values() ?? []) {
        for (const pane of mgr.getPanes?.() ?? []) {
          if (pane?.container?.dataset?.ptyId === ptyId) {
            const ta = pane.container.querySelector(
              '.xterm-helper-textarea'
            ) as HTMLTextAreaElement | null
            if (!ta) {
              throw new Error('no helper textarea on the target pane')
            }
            ta.value = text
            ta.dispatchEvent(new InputEvent('input', { data: text, bubbles: true }))
            return
          }
        }
      }
      throw new Error('no pane bound to the target ptyId')
    },
    { ptyId, text }
  )
}

/** CSS-px boxes of the panes in the SAME manager as `anchorPtyId`, sorted
 *  top-first. Scoped because hidden worktrees keep real (opacity-0) rects —
 *  the bootstrap pane must not be mistaken for one of the split halves. */
async function readPaneBoxes(page: Page, anchorPtyId: string): Promise<PaneBox[]> {
  return page.evaluate((anchorPtyId) => {
    const managers = (
      window as unknown as {
        __paneManagers?: Map<string, { getPanes?: () => { container?: HTMLElement | null }[] }>
      }
    ).__paneManagers
    const boxes: { ptyId: string; top: number; left: number; width: number; height: number }[] = []
    for (const mgr of managers?.values() ?? []) {
      const panes = mgr.getPanes?.() ?? []
      if (!panes.some((pane) => pane?.container?.dataset?.ptyId === anchorPtyId)) {
        continue
      }
      for (const pane of panes) {
        const el = pane?.container
        const ptyId = el?.dataset?.ptyId
        if (!el || !ptyId) {
          continue
        }
        const rect = el.getBoundingClientRect()
        if (rect.width > 0 && rect.height > 0) {
          boxes.push({
            ptyId,
            top: rect.top,
            left: rect.left,
            width: rect.width,
            height: rect.height
          })
        }
      }
    }
    return boxes.sort((a, b) => a.top - b.top)
  }, anchorPtyId)
}

/** One synchronized readback of the overlay canvas band ABOVE the pane's top
 *  clip line and the pane canvas rows just BELOW it. Both are plain main-thread
 *  2d canvases here (CPU drawer forced), so getImageData is exact. */
async function readSpillSeamProbe(page: Page, ptyId: string): Promise<SpillSeamProbe | null> {
  return page.evaluate(
    ({ ptyId, bandPx, alphaMin, bgDiffMin }) => {
      const managers = (
        window as unknown as {
          __paneManagers?: Map<string, { getPanes?: () => { container?: HTMLElement | null }[] }>
        }
      ).__paneManagers
      let paneEl: HTMLElement | null = null
      for (const mgr of managers?.values() ?? []) {
        for (const pane of mgr.getPanes?.() ?? []) {
          if (pane?.container?.dataset?.ptyId === ptyId) {
            paneEl = pane.container
          }
        }
      }
      const overlay = document.querySelector(
        '[data-testid="aterm-effects-spill-overlay"]'
      ) as HTMLCanvasElement | null
      const canvas = paneEl?.querySelector(
        '[data-testid="aterm-canvas"]'
      ) as HTMLCanvasElement | null
      if (!paneEl || !overlay || !canvas || overlay.width === 0 || canvas.width === 0) {
        return null
      }
      const overlayCtx = overlay.getContext('2d')
      const paneCtx = canvas.getContext('2d')
      if (!overlayCtx || !paneCtx) {
        return null
      }
      const dpr = window.devicePixelRatio || 1
      const overlayRect = overlay.getBoundingClientRect()
      const paneRect = paneEl.getBoundingClientRect()
      const canvasRect = canvas.getBoundingClientRect()
      // Shared x-extent of pane, its canvas, and the overlay, in device px.
      const xStart = Math.max(paneRect.left, canvasRect.left, overlayRect.left)
      const xEnd = Math.min(paneRect.right, canvasRect.right, overlayRect.right)
      const width = Math.floor((xEnd - xStart) * dpr)
      const clipTopOverlayY = Math.round((paneRect.top - overlayRect.top) * dpr)
      const clipTopCanvasY = Math.round((paneRect.top - canvasRect.top) * dpr)
      if (width <= 0 || clipTopOverlayY - bandPx < 0 || clipTopCanvasY < 0) {
        return null
      }
      const overlayX0 = Math.round((xStart - overlayRect.left) * dpr)
      const canvasX0 = Math.round((xStart - canvasRect.left) * dpr)
      const band = overlayCtx.getImageData(overlayX0, clipTopOverlayY - bandPx, width, bandPx)
      let overlayLit = 0
      for (let i = 3; i < band.data.length; i += 4) {
        if (band.data[i] > alphaMin) {
          overlayLit++
        }
      }
      // Pane side: two rows just below the clip line (chrome-band rows inside
      // the clip — no glyphs there, so any deviation from the seeded theme bg
      // is emission light).
      const bgParts = (canvas.dataset.atermBg ?? '')
        .split(',')
        .map((part) => Number.parseInt(part, 10))
      const paneBand = paneCtx.getImageData(canvasX0, clipTopCanvasY, width, 2)
      const litCols: number[] = []
      for (let x = 0; x < width; x++) {
        for (let row = 0; row < 2; row++) {
          const o = (row * width + x) * 4
          if (
            Math.abs(paneBand.data[o] - (bgParts[0] ?? 0)) > bgDiffMin ||
            Math.abs(paneBand.data[o + 1] - (bgParts[1] ?? 0)) > bgDiffMin ||
            Math.abs(paneBand.data[o + 2] - (bgParts[2] ?? 0)) > bgDiffMin
          ) {
            litCols.push(x)
            break
          }
        }
      }
      let gapRows = -1
      if (litCols.length > 0) {
        // Approximate continuity: within a widened window around the pane-side
        // lit columns, every overlay row just above the clip line must carry at
        // least one painted pixel (fire flickers, so exact columns can differ).
        const windowStart = Math.max(0, (litCols[0] ?? 0) - 24)
        const windowEnd = Math.min(width - 1, (litCols.at(-1) ?? 0) + 24)
        gapRows = 0
        for (let row = 1; row <= 3; row++) {
          const y = bandPx - row
          let rowLit = false
          for (let x = windowStart; x <= windowEnd; x++) {
            if (band.data[(y * width + x) * 4 + 3] > alphaMin) {
              rowLit = true
              break
            }
          }
          if (!rowLit) {
            gapRows++
          }
        }
      }
      return { overlayLit, paneLitCols: litCols.length, gapRows }
    },
    { ptyId, bandPx: OVERLAY_BAND_PX, alphaMin: OVERLAY_ALPHA_MIN, bgDiffMin: PANE_BG_DIFF_MIN }
  )
}

async function settleToByteStableFrames(page: Page, key: string, ptyId: string): Promise<void> {
  await expect
    .poll(
      async () => {
        if (!(await snapshotAtermCanvas(page, key, ptyId))) {
          return -1
        }
        await page.waitForTimeout(QUIET_WINDOW_MS)
        return countAtermChangedPixelsSince(page, key, ptyId)
      },
      { timeout: 30_000, message: `frames should settle to byte-identical (${key})` }
    )
    .toBe(0)
}

test.describe('aterm cross-pane spill overlay (in-process path)', () => {
  test('fire spill paints past the clip, stays seam-continuous, passes input, and unmounts off', async ({
    orcaPage
  }) => {
    test.skip(
      process.env.ORCA_E2E_ATERM_WORKER === '1',
      'in-process slice — the worker gate runs the worker-path describe instead'
    )
    // Serial pixel-poll phases (settle + two burn polls) can exceed the 120s default.
    test.setTimeout(240_000)
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

    // IN-PROCESS slice: worker off (also the harness default) + CPU drawer for
    // deterministic main-thread 2d readback of the pane canvas.
    await orcaPage.evaluate(() => {
      ;(window as unknown as { __atermWorkerRender?: boolean }).__atermWorkerRender = false
      ;(window as unknown as { __atermGpuDisabled?: boolean }).__atermGpuDisabled = true
    })

    // Enable ONLY the fire glow via settings (the keys the Terminal Engine panel
    // writes) BEFORE the panes open; blink/sparkles off so "settled" means
    // byte-identical and every lit pixel is attributable to the glow. Dark theme
    // pinned: additive fire light SATURATES against a light theme's near-white
    // background (the pane-side lit detector reads ~0 diff), so the seam probe
    // is only meaningful on a dark bg.
    await orcaPage.evaluate(async () => {
      await window.__store?.getState().updateSettings({
        theme: 'dark',
        terminalEffectsCursorGlow: true,
        terminalEffectsCursorGlowStyle: 'fire',
        terminalEffectsSparkleWords: false,
        terminalCursorBlink: false
      })
    })

    await orcaPage.getByRole('button', { name: 'New tab' }).click()
    await orcaPage
      .getByRole('menuitem', { name: /New Terminal/i })
      .first()
      .click()
    const firstPty = await waitForActivePanePtyId(orcaPage)
    await waitForAtermControllerByPtyId(orcaPage, firstPty)

    // Stack a second pane BELOW ('horizontal' split = column flex): the bottom
    // pane's head band then spills UP over the gap and the top pane — the
    // cross-pane region every assertion below reads.
    await splitActiveTerminalPane(orcaPage, 'horizontal')
    let panes: PaneBox[] = []
    await expect
      .poll(
        async () => {
          panes = await readPaneBoxes(orcaPage, firstPty)
          const tops = new Set(panes.map((pane) => Math.round(pane.top)))
          return panes.length >= 2 && tops.size >= 2
        },
        { timeout: 20_000, message: 'the split should yield two vertically stacked panes' }
      )
      .toBe(true)
    const topPane = panes[0]
    const bottomPane = panes.at(-1) ?? topPane
    await waitForAtermControllerByPtyId(orcaPage, bottomPane.ptyId)

    // Focus the bottom pane (the glow tracks the FOCUSED cursor).
    await orcaPage.mouse.click(
      bottomPane.left + bottomPane.width / 2,
      bottomPane.top + bottomPane.height / 2
    )

    // The spill layer mounts once a chrome-granted pane registers (capability +
    // paneKey bind land at the controller-attach edge).
    const overlay = orcaPage.locator('[data-testid="aterm-effects-spill-overlay"]')
    await expect(overlay, 'the spill overlay canvas should mount').toBeAttached({
      timeout: 20_000
    })

    // Let prompt output + the focus-driven glow settle so later paint is
    // attributable to typing (also absorbs lazy-font repaints).
    await settleToByteStableFrames(orcaPage, 'prompt-idle', bottomPane.ptyId)

    // (a) Ignite by typing (keystroke echo moves the cursor on the pane's TOP
    // prompt row; fire rises into the head band) and poll for painted overlay
    // pixels ABOVE the pane's top clip line. Re-type each attempt: the burn
    // decays in ~seconds, so one early keystroke could settle before a slow
    // first sample.
    let probe: SpillSeamProbe | null = null
    let keystroke = 0
    await expect
      .poll(
        async () => {
          await typeIntoPane(orcaPage, bottomPane.ptyId, 'aoe'[keystroke++ % 3] ?? 'a')
          await orcaPage.waitForTimeout(90)
          probe = await readSpillSeamProbe(orcaPage, bottomPane.ptyId)
          return probe !== null && probe.overlayLit > 20
        },
        {
          timeout: 60_000,
          message: 'the fire burn should paint overlay pixels above the clip line'
        }
      )
      .toBe(true)

    // (b) Seam continuity while burning: whenever the pane-side chrome rows just
    // below the clip line are lit across enough columns, none of the 3 overlay
    // rows just above the line may be fully transparent within the lit window.
    await expect
      .poll(
        async () => {
          await typeIntoPane(orcaPage, bottomPane.ptyId, 'aoe'[keystroke++ % 3] ?? 'a')
          await orcaPage.waitForTimeout(60)
          probe = await readSpillSeamProbe(orcaPage, bottomPane.ptyId)
          return probe !== null && probe.paneLitCols >= 6 && probe.gapRows === 0
        },
        {
          timeout: 60_000,
          message:
            'no fully-transparent overlay gap row above lit pane-side pixels (seam continuity)'
        }
      )
      .toBe(true)

    // (d) Input passthrough: click INSIDE the top pane, just above the bottom
    // pane's clip line — under the bottom pane's lit spill region on the
    // overlay (pointer-events: none). Focus must land on the pane below the
    // pixels, i.e. the TOP pane. 14 CSS px clears the divider/gap hit area.
    await typeIntoPane(orcaPage, bottomPane.ptyId, 'a') // keep the region lit
    await orcaPage.mouse.click(
      bottomPane.left + bottomPane.width / 2,
      Math.max(topPane.top + 1, bottomPane.top - 14)
    )
    await expect
      .poll(
        async () =>
          orcaPage.evaluate((ptyId) => {
            const managers = (
              window as unknown as {
                __paneManagers?: Map<
                  string,
                  { getPanes?: () => { container?: HTMLElement | null }[] }
                >
              }
            ).__paneManagers
            for (const mgr of managers?.values() ?? []) {
              for (const pane of mgr.getPanes?.() ?? []) {
                if (pane?.container?.dataset?.ptyId === ptyId) {
                  return pane.container.contains(document.activeElement)
                }
              }
            }
            return false
          }, topPane.ptyId),
        {
          timeout: 15_000,
          message: 'clicking through the spill region should focus the pane under it'
        }
      )
      .toBe(true)

    // (c) Glow off: every pane's chrome returns to 0/0, the registration seam
    // unregisters them all, and the layer unmounts (canvas leaves the DOM —
    // fully transparent by construction).
    await orcaPage.evaluate(async () => {
      await window.__store?.getState().updateSettings({ terminalEffectsCursorGlow: false })
    })
    await expect(overlay, 'disabling the glow should unmount the spill overlay').toHaveCount(0, {
      timeout: 20_000
    })
  })
})

// ── WORKER path (stage 4) ────────────────────────────────────────────────────

type ViewportBand = { left: number; top: number; width: number; height: number }

/** Painted-pixel count inside a viewport-CSS band of the WORKER overlay canvas.
 *  The element is transferred (getContext throws), but a placeholder canvas is a
 *  spec-valid drawImage SOURCE showing the last committed worker frame, so a
 *  readback canvas recovers exact RGBA (alpha included). null = not readable yet. */
async function readWorkerOverlayBandLit(page: Page, band: ViewportBand): Promise<number | null> {
  return page.evaluate(
    ({ band, alphaMin }) => {
      const overlay = document.querySelector(
        '[data-testid="aterm-effects-spill-overlay-worker"]'
      ) as HTMLCanvasElement | null
      if (!overlay) {
        return null
      }
      const rect = overlay.getBoundingClientRect()
      if (rect.width <= 0 || rect.height <= 0) {
        return null
      }
      // The worker sizes the committed bitmap to container × dpr (the geometry
      // tracker's box push); placeholder width attributes are not a reliable
      // mirror, so map CSS→device through dpr. Out-of-range source rects draw
      // clipped (transparent), which the polls tolerate.
      const scale = window.devicePixelRatio || 1
      const x = Math.round((band.left - rect.left) * scale)
      const y = Math.round((band.top - rect.top) * scale)
      const w = Math.round(band.width * scale)
      const h = Math.round(band.height * scale)
      if (w <= 0 || h <= 0 || x < 0 || y < 0) {
        return null
      }
      const read = document.createElement('canvas')
      read.width = w
      read.height = h
      const ctx = read.getContext('2d')
      if (!ctx) {
        return null
      }
      try {
        ctx.drawImage(overlay, x, y, w, h, 0, 0, w, h)
      } catch {
        // A never-committed/zero placeholder bitmap: not readable yet.
        return null
      }
      const data = ctx.getImageData(0, 0, w, h).data
      let lit = 0
      for (let i = 3; i < data.length; i += 4) {
        if (data[i] > alphaMin) {
          lit++
        }
      }
      return lit
    },
    { band, alphaMin: OVERLAY_ALPHA_MIN }
  )
}

/** Worker-path seam probe: the same overlay-above / pane-below continuity read
 *  as the in-process one, but both surfaces are worker-owned, so each is pulled
 *  through a drawImage readback of its placeholder first. */
async function readWorkerSeamProbe(page: Page, ptyId: string): Promise<SpillSeamProbe | null> {
  return page.evaluate(
    ({ ptyId, bandPx, alphaMin, bgDiffMin }) => {
      const managers = (
        window as unknown as {
          __paneManagers?: Map<string, { getPanes?: () => { container?: HTMLElement | null }[] }>
        }
      ).__paneManagers
      let paneEl: HTMLElement | null = null
      for (const mgr of managers?.values() ?? []) {
        for (const pane of mgr.getPanes?.() ?? []) {
          if (pane?.container?.dataset?.ptyId === ptyId) {
            paneEl = pane.container
          }
        }
      }
      const overlay = document.querySelector(
        '[data-testid="aterm-effects-spill-overlay-worker"]'
      ) as HTMLCanvasElement | null
      const canvas = paneEl?.querySelector(
        '[data-testid="aterm-canvas"]'
      ) as HTMLCanvasElement | null
      if (!paneEl || !overlay || !canvas) {
        return null
      }
      const readback = (
        source: HTMLCanvasElement,
        x: number,
        y: number,
        w: number,
        h: number
      ): Uint8ClampedArray | null => {
        const read = document.createElement('canvas')
        read.width = w
        read.height = h
        const ctx = read.getContext('2d')
        if (!ctx) {
          return null
        }
        try {
          ctx.drawImage(source, x, y, w, h, 0, 0, w, h)
        } catch {
          return null
        }
        return ctx.getImageData(0, 0, w, h).data
      }
      const overlayRect = overlay.getBoundingClientRect()
      const paneRect = paneEl.getBoundingClientRect()
      const canvasRect = canvas.getBoundingClientRect()
      // Both worker-owned bitmaps are pinned to their CSS boxes × dpr (canvas
      // CSS pinning + the overlay box push), so dpr is the CSS→device map.
      const oScale = window.devicePixelRatio || 1
      const cScale = oScale
      const xStart = Math.max(paneRect.left, canvasRect.left, overlayRect.left)
      const xEnd = Math.min(paneRect.right, canvasRect.right, overlayRect.right)
      const width = Math.floor((xEnd - xStart) * oScale)
      const clipTopOverlayY = Math.round((paneRect.top - overlayRect.top) * oScale)
      if (width <= 0 || clipTopOverlayY - bandPx < 0) {
        return null
      }
      const overlayX0 = Math.round((xStart - overlayRect.left) * oScale)
      const band = readback(overlay, overlayX0, clipTopOverlayY - bandPx, width, bandPx)
      if (!band) {
        return null
      }
      let overlayLit = 0
      for (let i = 3; i < band.length; i += 4) {
        if (band[i] > alphaMin) {
          overlayLit++
        }
      }
      // Pane side: two rows just below the clip line (chrome-band rows inside
      // the clip — no glyphs, so any deviation from the seeded theme bg is
      // emission light).
      const bgParts = (canvas.dataset.atermBg ?? '')
        .split(',')
        .map((part) => Number.parseInt(part, 10))
      const paneWidth = Math.floor((xEnd - xStart) * cScale)
      const clipTopCanvasY = Math.round((paneRect.top - canvasRect.top) * cScale)
      const canvasX0 = Math.round((xStart - canvasRect.left) * cScale)
      if (paneWidth <= 0 || clipTopCanvasY < 0) {
        return null
      }
      const paneBand = readback(canvas, canvasX0, clipTopCanvasY, paneWidth, 2)
      if (!paneBand) {
        return null
      }
      const litCols: number[] = []
      for (let x = 0; x < paneWidth; x++) {
        for (let row = 0; row < 2; row++) {
          const o = (row * paneWidth + x) * 4
          if (
            Math.abs(paneBand[o] - (bgParts[0] ?? 0)) > bgDiffMin ||
            Math.abs(paneBand[o + 1] - (bgParts[1] ?? 0)) > bgDiffMin ||
            Math.abs(paneBand[o + 2] - (bgParts[2] ?? 0)) > bgDiffMin
          ) {
            litCols.push(x)
            break
          }
        }
      }
      let gapRows = -1
      if (litCols.length > 0) {
        // Map pane-canvas columns into overlay columns before widening (the two
        // surfaces can carry different device scales).
        const toOverlayX = (paneX: number): number => Math.round((paneX / cScale) * oScale)
        const windowStart = Math.max(0, toOverlayX(litCols[0] ?? 0) - 24)
        const windowEnd = Math.min(width - 1, toOverlayX(litCols.at(-1) ?? 0) + 24)
        gapRows = 0
        for (let row = 1; row <= 3; row++) {
          const y = bandPx - row
          let rowLit = false
          for (let x = windowStart; x <= windowEnd; x++) {
            if (band[(y * width + x) * 4 + 3] > alphaMin) {
              rowLit = true
              break
            }
          }
          if (!rowLit) {
            gapRows++
          }
        }
      }
      return { overlayLit, paneLitCols: litCols.length, gapRows }
    },
    { ptyId, bandPx: OVERLAY_BAND_PX, alphaMin: OVERLAY_ALPHA_MIN, bgDiffMin: PANE_BG_DIFF_MIN }
  )
}

test.describe('aterm cross-pane spill overlay (worker path)', () => {
  test('worker spill escapes the clip, clears on pane close, and survives a worker restart', async ({
    orcaPage
  }) => {
    // Serial burn/clear/restart poll phases exceed the 120s default.
    test.setTimeout(300_000)
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

    // Force the PRODUCTION worker render path BEFORE the panes (the sibling
    // worker specs' convention; a no-op under the worker-ON gate where the
    // harness leaves the flag unset).
    await orcaPage.evaluate(() => {
      ;(window as unknown as { __atermWorkerRender?: boolean }).__atermWorkerRender = true
    })
    // Same effects profile as the in-process slice (dark theme pinned so the
    // additive fire light is attributable; blink/sparkles off = settled means
    // no further band changes).
    await orcaPage.evaluate(async () => {
      await window.__store?.getState().updateSettings({
        theme: 'dark',
        terminalEffectsCursorGlow: true,
        terminalEffectsCursorGlowStyle: 'fire',
        terminalEffectsSparkleWords: false,
        terminalCursorBlink: false
      })
    })

    await orcaPage.getByRole('button', { name: 'New tab' }).click()
    await orcaPage
      .getByRole('menuitem', { name: /New Terminal/i })
      .first()
      .click()
    const firstPty = await waitForActivePanePtyId(orcaPage)
    await waitForAtermControllerByPtyId(orcaPage, firstPty)
    await splitActiveTerminalPane(orcaPage, 'horizontal')
    let panes: PaneBox[] = []
    await expect
      .poll(
        async () => {
          panes = await readPaneBoxes(orcaPage, firstPty)
          const tops = new Set(panes.map((pane) => Math.round(pane.top)))
          return panes.length >= 2 && tops.size >= 2
        },
        { timeout: 20_000, message: 'the split should yield two vertically stacked panes' }
      )
      .toBe(true)
    const topPane = panes[0]
    let bottomPane = panes.at(-1) ?? topPane
    await waitForAtermControllerByPtyId(orcaPage, bottomPane.ptyId)

    // Focus the bottom pane (the glow tracks the FOCUSED cursor).
    await orcaPage.mouse.click(
      bottomPane.left + bottomPane.width / 2,
      bottomPane.top + bottomPane.height / 2
    )

    // The WORKER overlay canvas mounts once a worker-backed pane binds, and its
    // control is transferred to the shared render worker: getContext throws on
    // the element — the proof compositing happens off-main.
    const workerOverlay = orcaPage.locator('[data-testid="aterm-effects-spill-overlay-worker"]')
    await expect(workerOverlay, 'the worker spill overlay canvas should mount').toBeAttached({
      timeout: 30_000
    })
    const transferred = await orcaPage.evaluate(() => {
      const el = document.querySelector(
        '[data-testid="aterm-effects-spill-overlay-worker"]'
      ) as HTMLCanvasElement | null
      if (!el) {
        return false
      }
      try {
        el.getContext('2d')
        return false
      } catch {
        return true
      }
    })
    expect(transferred, 'the worker overlay canvas must be transferred (worker-owned)').toBe(true)

    const bandAbove = (pane: PaneBox): ViewportBand => ({
      left: pane.left + 8,
      top: pane.top - OVERLAY_BAND_PX,
      width: pane.width - 16,
      height: OVERLAY_BAND_PX
    })

    // Let the open/focus burn decay so later light is attributable to typing.
    await expect
      .poll(async () => readWorkerOverlayBandLit(orcaPage, bandAbove(bottomPane)), {
        timeout: 60_000,
        message: 'the head band should settle to transparent before ignition'
      })
      .toBe(0)

    // (a) Ignite by typing: the fire rises through the bottom pane's head band
    // into the window-space overlay ABOVE its clip line (over the gap + the
    // idle top pane). Re-type per attempt — the burn decays in ~seconds.
    let keystroke = 0
    await expect
      .poll(
        async () => {
          await typeIntoPane(orcaPage, bottomPane.ptyId, 'aoe'[keystroke++ % 3] ?? 'a')
          await orcaPage.waitForTimeout(90)
          const lit = await readWorkerOverlayBandLit(orcaPage, bandAbove(bottomPane))
          return lit !== null && lit > 20
        },
        {
          timeout: 60_000,
          message: 'the worker compositor should paint spill pixels above the clip line'
        }
      )
      .toBe(true)

    // (b) Seam continuity across the clip line while burning (worker readback).
    let probe: SpillSeamProbe | null = null
    await expect
      .poll(
        async () => {
          await typeIntoPane(orcaPage, bottomPane.ptyId, 'aoe'[keystroke++ % 3] ?? 'a')
          await orcaPage.waitForTimeout(60)
          probe = await readWorkerSeamProbe(orcaPage, bottomPane.ptyId)
          return probe !== null && probe.paneLitCols >= 6 && probe.gapRows === 0
        },
        {
          timeout: 60_000,
          message:
            'no fully-transparent overlay gap row above lit pane-side pixels (worker seam continuity)'
        }
      )
      .toBe(true)

    // (d) Input passthrough: click INSIDE the top pane, just above the bottom
    // pane's clip line — under the lit spill (pointer-events: none overlays).
    await typeIntoPane(orcaPage, bottomPane.ptyId, 'a') // keep the region lit
    await orcaPage.mouse.click(
      bottomPane.left + bottomPane.width / 2,
      Math.max(topPane.top + 1, bottomPane.top - 14)
    )
    await expect
      .poll(
        async () =>
          orcaPage.evaluate((ptyId) => {
            const managers = (
              window as unknown as {
                __paneManagers?: Map<
                  string,
                  { getPanes?: () => { container?: HTMLElement | null }[] }
                >
              }
            ).__paneManagers
            for (const mgr of managers?.values() ?? []) {
              for (const pane of mgr.getPanes?.() ?? []) {
                if (pane?.container?.dataset?.ptyId === ptyId) {
                  return pane.container.contains(document.activeElement)
                }
              }
            }
            return false
          }, topPane.ptyId),
        {
          timeout: 15_000,
          message: 'clicking through the worker spill region should focus the pane under it'
        }
      )
      .toBe(true)

    // Pane close mid-burn clears its strips: re-focus + re-ignite the bottom
    // pane, then close it while the band is still lit — the compositor must
    // clear its retained strips (and the region falls inside the expanded
    // survivor's clip, so nothing may repaint there).
    await orcaPage.mouse.click(
      bottomPane.left + bottomPane.width / 2,
      bottomPane.top + bottomPane.height / 2
    )
    const closedBand = bandAbove(bottomPane)
    await expect
      .poll(
        async () => {
          await typeIntoPane(orcaPage, bottomPane.ptyId, 'aoe'[keystroke++ % 3] ?? 'a')
          await orcaPage.waitForTimeout(90)
          const lit = await readWorkerOverlayBandLit(orcaPage, closedBand)
          return lit !== null && lit > 20
        },
        { timeout: 60_000, message: 're-ignition should light the band before the close' }
      )
      .toBe(true)
    await closeActiveTerminalPane(orcaPage)
    await expect
      .poll(async () => readWorkerOverlayBandLit(orcaPage, closedBand), {
        timeout: 20_000,
        message: 'closing the burning pane should clear its overlay strips'
      })
      .toBe(0)
    // The surviving top pane keeps the worker overlay alive (still registered).
    await expect(workerOverlay).toBeAttached()

    // Worker restart: retire the shared render worker through its REAL crash
    // path. Every live pane rebuilds in-process, so the worker canvas unbinds
    // and detaches; the spill layer itself stays (in-process re-registration).
    const epochsBefore = await orcaPage.evaluate(
      () =>
        (window as unknown as { __atermSpillCanvasEpochs?: number[] }).__atermSpillCanvasEpochs ??
        []
    )
    expect(epochsBefore.length, 'the first canvas generation should have shipped').toBeGreaterThan(
      0
    )
    await orcaPage.evaluate(() => {
      ;(
        window as unknown as { __atermRetireSharedRenderWorker?: () => void }
      ).__atermRetireSharedRenderWorker?.()
    })
    await expect(
      workerOverlay,
      'retiring the worker should unbind + unmount the worker overlay canvas'
    ).toHaveCount(0, { timeout: 30_000 })

    // A NEW pane now respawns a fresh shared worker; its spill bind must ship a
    // FRESH overlay canvas under a HIGHER epoch (the dead-epoch guard's other
    // half) and paint spill again — worker restart re-establishes spill.
    await splitActiveTerminalPane(orcaPage, 'horizontal')
    await expect
      .poll(
        async () => {
          panes = await readPaneBoxes(orcaPage, firstPty)
          return panes.length >= 2
        },
        { timeout: 20_000, message: 'the respawn split should yield a second pane' }
      )
      .toBe(true)
    bottomPane = panes.at(-1) ?? bottomPane
    await waitForAtermControllerByPtyId(orcaPage, bottomPane.ptyId)
    await expect(
      workerOverlay,
      'the respawned worker should re-establish the spill canvas'
    ).toBeAttached({ timeout: 30_000 })
    const epochAdvanced = await orcaPage.evaluate((before) => {
      const epochs =
        (window as unknown as { __atermSpillCanvasEpochs?: number[] }).__atermSpillCanvasEpochs ??
        []
      return epochs.length > before.length && (epochs.at(-1) ?? 0) > (before.at(-1) ?? 0)
    }, epochsBefore)
    expect(epochAdvanced, 'the respawned canvas generation must carry a higher epoch').toBe(true)
    await orcaPage.mouse.click(
      bottomPane.left + bottomPane.width / 2,
      bottomPane.top + bottomPane.height / 2
    )
    keystroke = 0
    await expect
      .poll(
        async () => {
          await typeIntoPane(orcaPage, bottomPane.ptyId, 'aoe'[keystroke++ % 3] ?? 'a')
          await orcaPage.waitForTimeout(90)
          const lit = await readWorkerOverlayBandLit(orcaPage, bandAbove(bottomPane))
          return lit !== null && lit > 20
        },
        {
          timeout: 60_000,
          message: 'the respawned worker should composite spill above the clip line again'
        }
      )
      .toBe(true)

    // Glow off: every pane unregisters (worker AND in-process) and the whole
    // layer unmounts — both canvases leave the DOM, cleared by construction.
    await orcaPage.evaluate(async () => {
      await window.__store?.getState().updateSettings({ terminalEffectsCursorGlow: false })
    })
    await expect(
      orcaPage.locator('[data-testid="aterm-effects-spill-overlay"]'),
      'disabling the glow should unmount the spill layer'
    ).toHaveCount(0, { timeout: 20_000 })
    await expect(workerOverlay).toHaveCount(0)
  })
})
