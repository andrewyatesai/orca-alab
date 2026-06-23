import { test, expect } from './helpers/orca-app'
import { waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'
import { atermCanvasReady, readAtermPixel, readAtermRgba } from './helpers/aterm-canvas-pixels'
import { writeFileSync } from 'node:fs'

// Proves Phase 1 of the aterm in-page renderer: THEME, SCROLLBACK SCROLL, and
// SELECTION+COPY for the aterm-rendered terminal pane (behind the experimental
// flag). Drives the REAL Electron app: opens an aterm pane, asserts the canvas
// background matches the seeded theme, then exercises wheel scroll and a mouse
// drag → selection → clipboard via the AtermPaneController exposed on the pane.
//
// Note on headless geometry: ORCA_E2E_HEADLESS keeps the window hidden, so DOM
// layout reports a 0x0 canvas rect (clientWidth/getBoundingClientRect == 0) even
// though the device-pixel framebuffer is fully sized. Bulk output is fed through
// the controller's process() (the exact path the PTY output mirror uses) so the
// scroll assertion is deterministic, and synthetic drag coordinates are passed
// canvas-relative (rect.left/top are 0) so they map to real grid cells.

type AtermControllerProbe = {
  process: (data: string) => void
  displayOffset: () => number
  scrollLines: (delta: number) => void
  selectionText: () => string
}

function findActiveController(): AtermControllerProbe {
  const managers = (window as unknown as { __paneManagers?: Map<string, unknown> }).__paneManagers
  if (!managers) {
    throw new Error('no pane managers')
  }
  for (const manager of managers.values()) {
    const m = manager as {
      getActivePane?: () => { atermController?: AtermControllerProbe | null } | null
      getPanes?: () => { atermController?: AtermControllerProbe | null }[]
    }
    const pane = m.getActivePane?.() ?? m.getPanes?.()[0] ?? null
    if (pane?.atermController) {
      return pane.atermController
    }
  }
  throw new Error('no aterm controller on any pane')
}

test.describe('aterm in-page renderer (Phase 1)', () => {
  test('theme, scrollback scroll, and selection+copy', async ({ orcaPage }) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

    // Turn the aterm renderer on BEFORE the pane that will use it is created.
    await orcaPage.evaluate(() => {
      ;(window as unknown as { __atermRendererEnabled?: boolean }).__atermRendererEnabled = true
    })

    await orcaPage.getByRole('button', { name: 'New tab' }).click()
    await orcaPage
      .getByRole('menuitem', { name: /New Terminal/i })
      .first()
      .click()

    const canvas = orcaPage.locator('[data-testid="aterm-canvas"]').first()
    await expect(canvas, 'aterm canvas should mount for the new pane').toBeAttached({
      timeout: 20_000
    })
    await waitForActivePanePtyId(orcaPage)

    // --- THEME ---------------------------------------------------------------
    // Assert a true background cell MATCHES orca's CONFIGURED terminal theme bg,
    // not merely "is dark". The expected bg is resolved INDEPENDENTLY in-page via
    // window.__resolveAtermThemeBg — the same resolveEffectiveTerminalAppearance →
    // composeActiveTerminalTheme pipeline the renderer seeds from, read fresh from
    // the store — so this fails if the renderer painted a bg that does NOT match
    // orca's configured theme (no reliance on the self-echoed data-aterm-bg).
    // Sample bottom-right (an empty cell on a fresh pane; the top-left cell holds
    // the block cursor, which would mask the bg).
    // The grid canvas may be GPU-owned (webgl2) or CPU-owned (2d); read pixels via
    // whichever (gl.readPixels / getImageData) through the shared helpers.
    await expect
      .poll(
        async () => {
          const ready = await atermCanvasReady(orcaPage)
          const hasResolver = await orcaPage.evaluate(() =>
            typeof (window as unknown as { __resolveAtermThemeBg?: () => unknown })
              .__resolveAtermThemeBg === 'function'
          )
          return ready && hasResolver ? true : null
        },
        {
          timeout: 20_000,
          message: 'aterm canvas should have a painted bg + the theme-bg resolver'
        }
      )
      .not.toBeNull()

    const buffer = await readAtermRgba(orcaPage)
    expect(buffer, 'should read the aterm canvas buffer').not.toBeNull()
    // Bottom-right pixel (top-left coords): an empty cell, free of the row-0/col-0
    // cursor block. readAtermPixel flips Y for the GPU swapchain.
    const pixel = await readAtermPixel(orcaPage, buffer!.w - 1, buffer!.h - 1)
    const bgProbe = await orcaPage.evaluate(
      (px) => {
        const resolve = (
          window as unknown as { __resolveAtermThemeBg?: () => [number, number, number] }
        ).__resolveAtermThemeBg
        const c = document.querySelector(
          '[data-testid="aterm-canvas"]'
        ) as HTMLCanvasElement | null
        if (!resolve || !px) {
          return null
        }
        // Resolve the configured theme bg through the REAL pipeline, independently
        // of whatever the renderer painted. Cross-check against the self-echoed
        // data-aterm-bg (NOT the assertion source) only for diagnostics.
        const expected = resolve()
        const raw = c?.dataset.atermBg
        const echoed = raw ? (raw.split(',').map((n) => Number(n)) as number[]) : undefined
        return { pixel: px as number[], expected, echoed }
      },
      pixel
    )
    expect(bgProbe, 'should read the canvas bg pixel + the resolved theme bg').not.toBeNull()
    const bgPixel = bgProbe!.pixel
    expect(bgPixel.every((v) => v >= 0 && v <= 255)).toBe(true)
    expect(bgProbe!.expected, 'theme-bg resolver should return an RGB triplet').toBeTruthy()
    const expectedBg = bgProbe!.expected
    // An empty cell's background must MATCH the configured theme bg within a small
    // tolerance (CPU rasterizer + any sub-pixel blend can nudge a channel a hair).
    const TOLERANCE = 6
    for (let ch = 0; ch < 3; ch++) {
      expect(
        Math.abs(bgPixel[ch] - expectedBg[ch]),
        `bg pixel channel ${ch} (${bgPixel}) should match the configured theme bg (${expectedBg})`
      ).toBeLessThanOrEqual(TOLERANCE)
    }
    // Cross-check (not the source of truth): the renderer's self-echoed seed
    // should agree with the independently-resolved theme bg.
    if (bgProbe!.echoed) {
      for (let ch = 0; ch < 3; ch++) {
        expect(
          Math.abs(bgProbe!.echoed[ch] - expectedBg[ch]),
          'the self-echoed seed should agree with the resolved theme bg'
        ).toBeLessThanOrEqual(TOLERANCE)
      }
    }

    // --- SCROLLBACK SCROLL ---------------------------------------------------
    // Feed many lines through the controller (the PTY-output mirror's path), then
    // dispatch a wheel-up over the canvas and assert the viewport scrolled into
    // history (display offset > 0).
    const offsetAfterWheel = await orcaPage.evaluate((findSrc: string) => {
      // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
      const find = new Function(`return (${findSrc})()`) as () => AtermControllerProbe
      const ctrl = find()
      let bulk = ''
      for (let i = 0; i < 300; i++) {
        bulk += `scrollback line ${i}\r\n`
      }
      ctrl.process(bulk)
      const atBottom = ctrl.displayOffset()
      const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement
      // deltaMode 1 (lines), negative deltaY = wheel up = reveal older history.
      c.dispatchEvent(
        new WheelEvent('wheel', { deltaY: -40, deltaMode: 1, bubbles: true, cancelable: true })
      )
      return { atBottom, after: ctrl.displayOffset() }
    }, findActiveController.toString())

    expect(offsetAfterWheel.atBottom, 'live output snaps the viewport to the bottom').toBe(0)
    expect(
      offsetAfterWheel.after,
      'wheel-up should scroll the viewport into scrollback'
    ).toBeGreaterThan(0)

    // --- SELECTION + COPY ----------------------------------------------------
    // Snap to bottom, print known rows, then drag across them with synthetic
    // mouse events and assert the selection text + clipboard captured content.
    const selection = await orcaPage.evaluate((findSrc: string) => {
      // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
      const find = new Function(`return (${findSrc})()`) as () => AtermControllerProbe
      const ctrl = find()
      ctrl.scrollLines(-100000) // snap to bottom
      let rows = ''
      for (let i = 0; i < 6; i++) {
        rows += `ATERMSELECT_ROW_${i}__________\r\n`
      }
      ctrl.process(rows)

      const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement
      const dpr = window.devicePixelRatio || 1
      // Hidden-window layout reports rect.left/top == 0, so pass canvas-relative
      // client coordinates (device pixels / dpr). The controller maps these back
      // to grid cells via getBoundingClientRect (left/top 0) * dpr.
      const mk = (type: string, deviceX: number, deviceY: number): MouseEvent =>
        new MouseEvent(type, {
          button: 0,
          buttons: 1,
          clientX: deviceX / dpr,
          clientY: deviceY / dpr,
          bubbles: true,
          cancelable: true
        })
      c.dispatchEvent(mk('mousedown', 4, 4))
      c.dispatchEvent(mk('mousemove', 400, 80))
      window.dispatchEvent(mk('mouseup', 400, 80))

      return {
        text: ctrl.selectionText(),
        copied: (window as unknown as { __atermLastCopied?: string }).__atermLastCopied ?? ''
      }
    }, findActiveController.toString())

    expect(selection.text.length, 'a canvas drag should produce a non-empty selection').toBeGreaterThan(0)
    expect(
      selection.copied.length,
      'mouseup should copy the selection to the clipboard'
    ).toBeGreaterThan(0)
    expect(selection.copied, 'clipboard should hold the selected text').toBe(selection.text)

    // Screenshot the final canvas state.
    const dataUrl = await orcaPage.evaluate(() => {
      const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement | null
      return c ? c.toDataURL('image/png') : ''
    })
    expect(dataUrl.startsWith('data:image/png;base64,')).toBe(true)
    writeFileSync('/tmp/aterm-phase1.png', Buffer.from(dataUrl.split(',')[1], 'base64'))
  })
})
