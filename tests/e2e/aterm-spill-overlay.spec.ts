import { test, expect } from './helpers/orca-app'
import type { Page } from '@stablyai/playwright-test'
import { splitActiveTerminalPane, waitForActivePanePtyId } from './helpers/terminal'
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
// IN-PROCESS slice by design: the worker path's spill compositor is stage 4, so
// this spec forces __atermWorkerRender = false itself (the default harness does
// too; the explicit write keeps it honest under ORCA_E2E_ATERM_WORKER=1) and
// forces the CPU drawer for deterministic 2d pixel readback. It is deliberately
// NOT in run-aterm-worker-on-e2e.mjs — the worker gate must stay a worker gate.

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
