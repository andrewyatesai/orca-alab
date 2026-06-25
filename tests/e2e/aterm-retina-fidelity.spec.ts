// PROVES the aterm renderer rasterizes at the host's REAL device pixel ratio, so
// glyphs stay crisp on a Retina (devicePixelRatio=2) display. The rest of the e2e
// suite runs headless at dpr=1, so the dpr=2 code path a Retina-Mac user actually
// sees is never exercised. This spec forces the Chromium device scale factor to 2
// (so window.devicePixelRatio reports 2 in the renderer) and asserts the HARD,
// CI-checkable invariant that catches "blurry text":
//
//   the aterm <canvas> backing store (canvas.width/height, in DEVICE pixels) MUST
//   equal round(CSS_box_size * devicePixelRatio).
//
// If the engine were pinned at dpr=1 while displayed at dpr=2, the backing store
// would be HALF resolution (round(CSS * 1)) and the browser would upscale it 2x —
// the exact upscale-blur this guards against. The pass/fail is the numeric
// invariant; the saved PNG is for human inspection only.
//
// HOW the forced dpr=2 takes effect: this spec opts in via the orca-app fixture's
// launchEnv (test-scoped), which the fixture passes to getOrcaElectronLaunchArgs.
// That helper appends Chromium's `--force-device-scale-factor=2` switch. Under
// Orca's "headless" mode (which only suppresses mainWindow.show(); the
// BrowserWindow + compositor are real, NOT Chromium --headless), that switch makes
// window.devicePixelRatio report 2 in the renderer. Verified: it works. The opt-in
// is scoped to this test, so other specs in the same worker stay at dpr=1.

import { test, expect } from './helpers/orca-app'
import { waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveAtermController } from './helpers/aterm-controller'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'
import { writeFileSync } from 'node:fs'

const RETINA_PNG = '/tmp/aterm-retina-fidelity.png'

test.describe('aterm Retina (devicePixelRatio=2) fidelity', () => {
  // Force the Chromium device scale factor to 2 for THIS spec only. launchEnv is a
  // test-scoped fixture option, so it never leaks the forced DPR to other specs.
  test.use({ launchEnv: { ORCA_E2E_FORCE_DPR: '2' } })

  test('aterm canvas backing store matches CSS size × devicePixelRatio (no upscale blur)', async ({
    orcaPage
  }) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

    // Turn the aterm renderer on BEFORE the pane that will use it is created.
    await orcaPage.evaluate(() => {
      ;(window as unknown as { __atermRendererEnabled?: boolean }).__atermRendererEnabled = true
    })

    // Sanity: confirm the forced device scale factor actually took effect. Under
    // some headless modes the switch is ignored; if dpr reports 1 we skip with a
    // clear message instead of a false pass (the invariant is meaningless at dpr=1).
    const dpr = await orcaPage.evaluate(() => window.devicePixelRatio)
    test.skip(
      dpr !== 2,
      `forced device scale factor did not take effect: window.devicePixelRatio=${dpr} (expected 2). ` +
        `The --force-device-scale-factor=2 switch was not honored by this headless Electron runner; ` +
        `the Retina invariant cannot be exercised. Investigate an alternative (e.g. CDP ` +
        `Emulation.setDeviceMetricsOverride deviceScaleFactor:2) before trusting this spec.`
    )
    expect(dpr, 'forced devicePixelRatio should be 2').toBe(2)

    // New terminal tab → its pane is rendered by aterm.
    await orcaPage.getByRole('button', { name: 'New tab' }).click()
    await orcaPage
      .getByRole('menuitem', { name: /New Terminal/i })
      .first()
      .click()

    const canvas = orcaPage.locator('[data-testid="aterm-canvas"]').first()
    // The e2e window is hidden (ORCA_E2E_HEADLESS), so assert attached, not visible.
    await expect(canvas, 'aterm canvas should mount for the new pane').toBeAttached({
      timeout: 20_000
    })

    // Wait for the PTY and the async aterm controller (wasm/font/GPU load) so the
    // canvas is actually sized to the laid-out pane before we read its dimensions.
    // Scope every read to the ACTIVE pane's canvas (the one bound to this ptyId):
    // the seeded baseline tab leaves a hidden background canvas whose
    // getBoundingClientRect is 0×0, so a DOM-first-match would measure the wrong,
    // unlaid-out pane. The active pane is the laid-out one with a real CSS box.
    const ptyId = await waitForActivePanePtyId(orcaPage)
    await waitForActiveAtermController(orcaPage)

    // Locate the active pane's canvas by ptyId (its container carries data-pty-id).
    const findActiveCanvasSrc = `(ptyId) => {
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

    // Wait until the active pane's canvas has a non-zero CSS box (laid out + painted
    // at least once) before reading the backing store; an unpainted canvas reports 0.
    await expect
      .poll(
        async () =>
          orcaPage.evaluate(
            ({ ptyId, findSrc }) => {
              // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
              const find = new Function(`return (${findSrc})`)() as (
                id: string
              ) => HTMLCanvasElement | null
              const c = find(ptyId)
              if (!c) {
                return 0
              }
              const rect = c.getBoundingClientRect()
              return Math.min(rect.width, rect.height, c.width, c.height)
            },
            { ptyId, findSrc: findActiveCanvasSrc }
          ),
        {
          timeout: 20_000,
          message: 'active aterm canvas should have a non-zero CSS box and backing store'
        }
      )
      .toBeGreaterThan(0)

    // THE KEY MEASUREMENT: backing store (device px) vs CSS box (logical px), read
    // from the ACTIVE pane's canvas.
    const m = await orcaPage.evaluate(
      ({ ptyId, findSrc }) => {
        // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
        const find = new Function(`return (${findSrc})`)() as (
          id: string
        ) => HTMLCanvasElement | null
        const c = find(ptyId)
        if (!c) {
          return null
        }
        const rect = c.getBoundingClientRect()
        return {
          dpr: window.devicePixelRatio,
          backingWidth: c.width,
          backingHeight: c.height,
          cssWidth: rect.width,
          cssHeight: rect.height
        }
      },
      { ptyId, findSrc: findActiveCanvasSrc }
    )
    expect(
      m,
      'active aterm canvas must be present to measure backing store vs CSS box'
    ).not.toBeNull()

    const expectedWidth = Math.round(m!.cssWidth * m!.dpr)
    const expectedHeight = Math.round(m!.cssHeight * m!.dpr)
    // The single-dpr (blurry) resolution the bug would produce, for the message.
    const blurryWidth = Math.round(m!.cssWidth * 1)
    const blurryHeight = Math.round(m!.cssHeight * 1)

    // eslint-disable-next-line no-console
    console.log(
      `[aterm-retina] dpr=${m!.dpr} css=${m!.cssWidth.toFixed(1)}x${m!.cssHeight.toFixed(1)} ` +
        `backing=${m!.backingWidth}x${m!.backingHeight} expected(@dpr)=${expectedWidth}x${expectedHeight} ` +
        `blurry(@1)=${blurryWidth}x${blurryHeight}`
    )

    // ±1 px tolerance for sub-pixel CSS-box rounding. A FAIL here means the backing
    // store is dpr=1 resolution while the canvas is displayed at dpr=2 — the browser
    // upscales the half-res framebuffer 2x and the user sees blurry text.
    expect(
      Math.abs(m!.backingWidth - expectedWidth),
      `aterm canvas backing-store WIDTH is upscaled/blurry: backing=${m!.backingWidth}px but the ` +
        `CSS box is ${m!.cssWidth.toFixed(1)}px at devicePixelRatio=${m!.dpr}, so the framebuffer ` +
        `should be ${expectedWidth}px. It is ~${blurryWidth}px (dpr=1 resolution), meaning the engine ` +
        `rasterized at dpr=1 and the browser upscales it ${m!.dpr}x → blurry glyphs on Retina.`
    ).toBeLessThanOrEqual(1)
    expect(
      Math.abs(m!.backingHeight - expectedHeight),
      `aterm canvas backing-store HEIGHT is upscaled/blurry: backing=${m!.backingHeight}px but the ` +
        `CSS box is ${m!.cssHeight.toFixed(1)}px at devicePixelRatio=${m!.dpr}, so the framebuffer ` +
        `should be ${expectedHeight}px. It is ~${blurryHeight}px (dpr=1 resolution), meaning the engine ` +
        `rasterized at dpr=1 and the browser upscales it ${m!.dpr}x → blurry glyphs on Retina.`
    ).toBeLessThanOrEqual(1)

    // Save the active pane's canvas to a PNG for human inspection (pass/fail is the
    // numeric invariant above, not the image).
    const dataUrl = await orcaPage.evaluate(
      ({ ptyId, findSrc }) => {
        // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
        const find = new Function(`return (${findSrc})`)() as (
          id: string
        ) => HTMLCanvasElement | null
        const c = find(ptyId)
        return c ? c.toDataURL('image/png') : ''
      },
      { ptyId, findSrc: findActiveCanvasSrc }
    )
    if (dataUrl.startsWith('data:image/png;base64,')) {
      writeFileSync(RETINA_PNG, Buffer.from(dataUrl.split(',')[1], 'base64'))
      // eslint-disable-next-line no-console
      console.log(`[aterm-retina] wrote ${RETINA_PNG}`)
    }
  })
})
