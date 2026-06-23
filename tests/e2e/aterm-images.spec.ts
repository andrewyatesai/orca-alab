import { test, expect } from './helpers/orca-app'
import { waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'

// Proves the aterm in-page renderer DISPLAYS INLINE IMAGES on its <canvas> for
// BOTH supported protocols:
//   (a) iTerm2 OSC 1337 `File=inline=1:<base64 PNG>` — a tiny solid RED 8x8 PNG.
//   (b) Sixel DCS (`ESC P q … ST`) — a solid GREEN raster block.
// For each, we feed the sequence through the pane's aterm controller (the same
// process() seam the PTY output mirror uses) and assert the canvas contains a
// CLUSTER of pixels of the image's color that is neither the theme bg nor fg —
// i.e. the colored region is the blitted image, not rasterized text. Drives the
// REAL Electron app.
//
// Headless note (ORCA_E2E_HEADLESS): the window is hidden so DOM layout reports
// a 0x0 rect. Output is fed through the controller's process() so the assertion
// is deterministic without OS focus/geometry. The image sequences are pure ASCII
// (base64 / sixel data) plus C0 ESC framing, so the string process() path (which
// UTF-8 encodes to the engine) carries them verbatim.

type AtermImageControllerProbe = {
  process: (data: string) => void
  scrollLines: (delta: number) => void
}

function findActiveController(): AtermImageControllerProbe {
  const managers = (window as unknown as { __paneManagers?: Map<string, unknown> }).__paneManagers
  if (!managers) {
    throw new Error('no pane managers')
  }
  for (const manager of managers.values()) {
    const m = manager as {
      getActivePane?: () => { atermController?: AtermImageControllerProbe | null } | null
      getPanes?: () => { atermController?: AtermImageControllerProbe | null }[]
    }
    const pane = m.getActivePane?.() ?? m.getPanes?.()[0] ?? null
    if (pane?.atermController) {
      return pane.atermController
    }
  }
  throw new Error('no aterm controller on any pane')
}

// A known-good 8x8 PURE-RED (255,0,0) RGB PNG, base64-encoded. Generated with a
// minimal hand-rolled PNG encoder (IHDR truecolor + zlib IDAT of red rows). The
// renderer (aterm-render, pure-Rust `png` crate) decodes + resamples it to fill
// the image's cell footprint, so a red cluster on the canvas proves the blit.
const RED_PNG_B64 =
  'iVBORw0KGgoAAAANSUhEUgAAAAgAAAAICAIAAABLbSncAAAAEklEQVR4nGP4z8CAFWEXHbQSACj/P8Fu7N9hAAAAAElFTkSuQmCC'

// iTerm2 OSC 1337 inline-image sequence: ESC ] 1337 ; File=inline=1 : <b64> BEL.
const RED_ITERM2 = `]1337;File=inline=1:${RED_PNG_B64}`

// A Sixel DCS that paints a solid GREEN block. Framing: ESC P q <body> ESC \.
//   "1;1;16;12 — DECGRA raster attributes: 16px wide, 12px tall (two 6px bands).
//   #1;2;0;100;0 — define color register 1 as RGB-percent (0,100,0) → (0,255,0).
//   #1 — select register 1.
//   !16~ — DECGRI: repeat the data byte '~' (all 6 rows set) 16 columns.
//   -    — graphics newline: advance to the next 6px band.
//   #1!16~ — fill the second band the same way.
// The `q` final byte after the (empty) DCS params is REQUIRED for the engine to
// route the body to the sixel decoder. ESC \ is the ST terminator.
const GREEN_SIXEL = 'Pq"1;1;16;12#1;2;0;100;0#1!16~-#1!16~\\'

type ColorProbe = {
  matched: number
  bg: [number, number, number]
}

test.describe('aterm inline images', () => {
  test('blits iTerm2 OSC-1337 and Sixel images to the canvas', async ({ orcaPage }) => {
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
    await expect(canvas, 'aterm canvas should mount').toBeAttached({ timeout: 20_000 })
    await waitForActivePanePtyId(orcaPage)

    // Feed a sequence, let two frames paint, then count canvas pixels matching the
    // target color within a tolerance. The bg sample (top-left) is returned so the
    // assertion can confirm the matched color is NOT the theme background.
    const countColor = async (
      sequence: string,
      target: [number, number, number],
      tol: number
    ): Promise<ColorProbe | null> =>
      orcaPage.evaluate(
        async (args: {
          findSrc: string
          sequence: string
          target: [number, number, number]
          tol: number
        }) => {
          // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
          const find = new Function(`return (${args.findSrc})()`) as () => {
            process: (d: string) => void
            scrollLines: (n: number) => void
          }
          const ctrl = find()
          ctrl.scrollLines(-100000) // snap to the live bottom
          ctrl.process(args.sequence)
          const c = document.querySelector(
            '[data-testid="aterm-canvas"]'
          ) as HTMLCanvasElement | null
          const ctx = c?.getContext('2d')
          if (!c || !ctx || !c.width || !c.height) {
            return null
          }
          const raf = (): Promise<void> =>
            new Promise((resolve) => requestAnimationFrame(() => resolve()))
          await raf()
          await raf()
          const d = ctx.getImageData(0, 0, c.width, c.height).data
          const [tr, tg, tb] = args.target
          let matched = 0
          for (let i = 0; i < d.length; i += 4) {
            if (
              Math.abs(d[i] - tr) <= args.tol &&
              Math.abs(d[i + 1] - tg) <= args.tol &&
              Math.abs(d[i + 2] - tb) <= args.tol
            ) {
              matched++
            }
          }
          // Sample the theme bg from the BOTTOM-RIGHT corner: the image is placed
          // at the cursor (top-left of a fresh grid), so the far corner is plain
          // background — the right reference to prove the matched color is the
          // image, not the theme bg.
          const last = d.length - 4
          return { matched, bg: [d[last], d[last + 1], d[last + 2]] as [number, number, number] }
        },
        { findSrc: findActiveController.toString(), sequence, target, tol }
      )

    // (a) iTerm2 OSC-1337 inline image: a solid RED PNG must paint red pixels.
    const redProbe = await countColor(RED_ITERM2, [255, 0, 0], 48)
    expect(redProbe, 'should read the canvas after the iTerm2 image').not.toBeNull()
    expect(
      redProbe!.bg,
      'the theme background must not itself be red (else the test is vacuous)'
    ).not.toEqual([255, 0, 0])
    expect(
      redProbe!.matched,
      `the iTerm2 OSC-1337 red PNG must blit a cluster of red pixels (got ${redProbe!.matched})`
    ).toBeGreaterThan(20)

    // (b) Sixel: a solid GREEN block must paint green pixels on the same canvas.
    const greenProbe = await countColor(GREEN_SIXEL, [0, 255, 0], 56)
    expect(greenProbe, 'should read the canvas after the sixel image').not.toBeNull()
    expect(
      greenProbe!.bg,
      'the theme background must not itself be green (else the test is vacuous)'
    ).not.toEqual([0, 255, 0])
    expect(
      greenProbe!.matched,
      `the Sixel green block must blit a cluster of green pixels (got ${greenProbe!.matched})`
    ).toBeGreaterThan(20)
  })
})
