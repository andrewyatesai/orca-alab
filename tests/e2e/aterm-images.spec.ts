import { test, expect } from './helpers/orca-app'
import { waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveAtermController } from './helpers/aterm-controller'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'
import { readAtermRgba } from './helpers/aterm-canvas-pixels'

// Proves the aterm in-page renderer DISPLAYS INLINE IMAGES on its <canvas> for
// ALL THREE supported protocols:
//   (a) iTerm2 OSC 1337 `File=inline=1:<base64 PNG>` — a tiny solid RED 8x8 PNG.
//   (b) Sixel DCS (`ESC P q … ST`) — a solid GREEN raster block.
//   (c) Kitty graphics (APC `_G` … ST) — a direct-transmission solid BLUE raster.
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

// Build a Kitty graphics direct-transmission sequence for a solid-color raw RGB
// (`f=24`) raster of `size`x`size` px, displayed at the cursor (`a=T`). Framing:
// ESC _ G <control> ; <base64 payload> ESC \ .
//   a=T — transmit AND display immediately (KittyAction::TransmitAndDisplay).
//   f=24 — packed RGB, 3 bytes/pixel (the engine expands to RGBA).
//   s=<w>,v=<h> — REQUIRED source pixel dims for raw formats (else build_kitty_image
//                 rejects the buffer); the payload MUST be exactly w*h*3 bytes.
//   t=d — direct medium: the payload IS the (base64) image data.
// The engine decodes this into the SAME RenderInput.images path sixel/iTerm2 use,
// so a solid raster blits a cluster of that color on the canvas. Pure ASCII, so it
// survives the controller's string process() (UTF-8 encoded to the engine).
function kittyRgbSolid(size: number, rgb: [number, number, number]): string {
  const raw = Buffer.alloc(size * size * 3)
  for (let i = 0; i < raw.length; i += 3) {
    raw[i] = rgb[0]
    raw[i + 1] = rgb[1]
    raw[i + 2] = rgb[2]
  }
  const b64 = raw.toString('base64')
  return `\x1b_Ga=T,f=24,s=${size},v=${size},t=d;${b64}\x1b\\`
}

// A 32x32 solid BLUE (0,0,255) Kitty raster — wide/tall enough to span several
// cells so the matched-pixel cluster is unambiguous.
const BLUE_KITTY = kittyRgbSolid(32, [0, 0, 255])

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
    // Wait for the async aterm controller (wasm/font/GPU load) so the in-page probe
    // below finds it — under parallel e2e load it can attach after the PTY binds.
    await waitForActiveAtermController(orcaPage)

    // Feed a sequence, let two frames paint, then count canvas pixels matching the
    // target color within a tolerance. The bg sample (top-left) is returned so the
    // assertion can confirm the matched color is NOT the theme background.
    const countColor = async (
      sequence: string,
      target: [number, number, number],
      tol: number
    ): Promise<ColorProbe | null> => {
      // Drive the controller (snap + process) in-page; the grid canvas may be
      // GPU-owned (webgl2) or CPU-owned (2d), so read pixels via the shared helper.
      await orcaPage.evaluate(
        async (args: { findSrc: string; sequence: string }) => {
          // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
          const find = new Function(`return (${args.findSrc})()`) as () => {
            process: (d: string) => void
            scrollLines: (n: number) => void
          }
          const ctrl = find()
          ctrl.scrollLines(-100000) // snap to the live bottom
          ctrl.process(args.sequence)
          const raf = (): Promise<void> =>
            new Promise((resolve) => requestAnimationFrame(() => resolve()))
          await raf()
          await raf()
        },
        { findSrc: findActiveController.toString(), sequence }
      )
      const read = await readAtermRgba(orcaPage)
      if (!read) {
        return null
      }
      const d = read.data
      const [tr, tg, tb] = target
      let matched = 0
      for (let i = 0; i < d.length; i += 4) {
        if (Math.abs(d[i] - tr) <= tol && Math.abs(d[i + 1] - tg) <= tol && Math.abs(d[i + 2] - tb) <= tol) {
          matched++
        }
      }
      // Sample the theme bg from the buffer's last pixel: the image is placed at
      // the cursor (top-left of a fresh grid), so the far corner is plain
      // background — the right reference to prove the matched color is the image,
      // not the theme bg. (Row order differs GPU vs CPU but the far corner is bg
      // in both.)
      const last = d.length - 4
      return { matched, bg: [d[last], d[last + 1], d[last + 2]] as [number, number, number] }
    }

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

    // (c) Kitty graphics (APC _G): a direct-transmission solid BLUE raster must
    // blit blue pixels on the same canvas, proving the engine routes Kitty images
    // into the shared RenderInput.images path (parity with sixel/iTerm2).
    const blueProbe = await countColor(BLUE_KITTY, [0, 0, 255], 56)
    expect(blueProbe, 'should read the canvas after the Kitty image').not.toBeNull()
    expect(
      blueProbe!.bg,
      'the theme background must not itself be blue (else the test is vacuous)'
    ).not.toEqual([0, 0, 255])
    expect(
      blueProbe!.matched,
      `the Kitty APC _G blue raster must blit a cluster of blue pixels (got ${blueProbe!.matched})`
    ).toBeGreaterThan(20)
  })
})
