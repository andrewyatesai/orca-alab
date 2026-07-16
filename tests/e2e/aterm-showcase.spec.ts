import { test, expect } from './helpers/orca-app'
import { waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveAtermController } from './helpers/aterm-controller'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'
import { writeFileSync } from 'node:fs'

// A holistic "experience" capture: renders a rich real-world frame through the
// aterm engine (true color, 256-color ramp, box-drawing table, CJK + emoji,
// programming ligatures, a Sixel image) and saves a PNG. Captured on the CPU
// path (clean toDataURL in headless); GPU output is pixel-identical (parity is
// proven separately in aterm-webgl.spec.ts), so this is representative of the
// live GPU experience. Asserts the frame has substantial rendered content.

type Probe = { process: (d: string) => void }
// Resolve controller AND canvas BY PTY ID — the identity the test drives.
// Positional lookups (manager order / first-canvas querySelector) target the
// bootstrap "Terminal 1" pane, which on GPU-capable hosts is webgl2-owned:
// its getContext('2d') is null and the payload never reaches the scanned pane.
function findController(ptyId: string): Probe {
  const managers = (window as unknown as { __paneManagers?: Map<string, unknown> }).__paneManagers
  for (const m of managers?.values() ?? []) {
    const mgr = m as {
      getPanes?: () => {
        atermController?: Probe | null
        container?: { dataset?: { ptyId?: string } }
      }[]
    }
    for (const pane of mgr.getPanes?.() ?? []) {
      if (pane?.container?.dataset?.ptyId === ptyId && pane.atermController) {
        return pane.atermController
      }
    }
  }
  throw new Error(`no aterm controller for pty ${ptyId}`)
}

// In-page source (new Function) for the canvas of the pane bound to ptyId.
const CANVAS_BY_PTY = `(ptyId) => {
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

// A solid 24x12 magenta Sixel block (deterministic, ASCII-only payload).
function sixelBlock(): string {
  const ESC = '\x1b'
  let s = `${ESC}Pq#0;2;100;0;100#0`
  for (let band = 0; band < 2; band++) {
    s += `#0${'~'.repeat(24)}$-`
  }
  return `${s}${ESC}\\`
}

test.describe('aterm showcase', () => {
  test('renders a rich real-world frame (color, box-drawing, unicode, ligatures, image)', async ({
    orcaPage
  }) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)
    // Force the CPU draw path so the headless toDataURL capture is clean; the
    // engine + output are identical to the GPU path (parity proven elsewhere).
    await orcaPage.evaluate(() => {
      ;(window as unknown as { __atermGpuDisabled?: boolean }).__atermGpuDisabled = true
    })
    // Cursor glow (default-on) grants window-space chrome that pads the frame around
    // the grid; this spec's band scans assume grid-anchored x/y, so pin glow off.
    await orcaPage.evaluate(async () => {
      await window.__store?.getState().updateSettings({ terminalEffectsCursorGlow: false })
    })
    await orcaPage.getByRole('button', { name: 'New tab' }).click()
    await orcaPage
      .getByRole('menuitem', { name: /New Terminal/i })
      .first()
      .click()
    const canvas = orcaPage.locator('[data-testid="aterm-canvas"]').first()
    await expect(canvas).toBeAttached({ timeout: 20_000 })
    const ptyId = await waitForActivePanePtyId(orcaPage)
    // Wait for the async aterm controller (wasm/font/GPU load) so the in-page probe
    // below finds it — under parallel e2e load it can attach after the PTY binds.
    await waitForActiveAtermController(orcaPage)

    const payload = [
      '\x1b[2J\x1b[H',
      '\x1b[1;38;5;81m aterm\x1b[0m \x1b[2min orca — GPU terminal\x1b[0m\r\n\r\n',
      // 256-color ramp
      Array.from({ length: 32 }, (_, i) => `\x1b[48;5;${16 + i * 6}m \x1b[0m`).join(''),
      '\r\n',
      // true-color gradient
      Array.from(
        { length: 32 },
        (_, i) => `\x1b[48;2;${i * 8};${128};${255 - i * 8}m \x1b[0m`
      ).join(''),
      '\r\n\r\n',
      // box-drawing table
      '\x1b[38;5;245m┌─────────────┬──────────┐\r\n',
      '│ \x1b[36mfeature\x1b[38;5;245m     │ \x1b[36mstatus\x1b[38;5;245m   │\r\n',
      '├─────────────┼──────────┤\r\n',
      '│ \x1b[0mligatures\x1b[38;5;245m   │ \x1b[32m✓ on\x1b[38;5;245m     │\r\n',
      '│ \x1b[0mGPU webgl\x1b[38;5;245m   │ \x1b[32m✓ on\x1b[38;5;245m     │\r\n',
      '└─────────────┴──────────┘\x1b[0m\r\n\r\n',
      // unicode + emoji
      ' CJK 你好世界  café  Ω≈ç√∫  🚀 🔥 ✨ 🦀\r\n\r\n',
      // ligatures
      ' \x1b[38;5;213mconst\x1b[0m f = (x) => x !== 0 && x === y; \x1b[2m// -> <= >= ==> |> .. ::\x1b[0m\r\n\r\n',
      ' image: ',
      sixelBlock(),
      '\r\n'
    ].join('')

    await orcaPage.evaluate(
      (args: { findSrc: string; payload: string; ptyId: string }) => {
        // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
        const find = new Function(`return (${args.findSrc})`)() as (id: string) => {
          process: (d: string) => void
        }
        find(args.ptyId).process(args.payload)
      },
      { findSrc: findController.toString(), payload, ptyId }
    )

    // Substantial rendered content (not a blank/near-blank frame).
    const nonBg = await expect
      .poll(
        async () =>
          orcaPage.evaluate(
            ({ findSrc, ptyId }) => {
              // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
              const findCanvas = new Function(`return (${findSrc})`)() as (
                id: string
              ) => HTMLCanvasElement | null
              const c = findCanvas(ptyId)
              const ctx = c?.getContext('2d')
              if (!c || !ctx || !c.width) {
                return 0
              }
              const d = ctx.getImageData(0, 0, c.width, c.height).data
              const bg = [d[0], d[1], d[2]]
              let n = 0
              for (let i = 0; i < d.length; i += 4) {
                if (d[i] !== bg[0] || d[i + 1] !== bg[1] || d[i + 2] !== bg[2]) {
                  n++
                }
              }
              return n
            },
            { findSrc: CANVAS_BY_PTY, ptyId }
          ),
        { timeout: 20_000, message: 'showcase frame should have rich content' }
      )
      .toBeGreaterThan(5000)
    void nonBg

    // CJK + emoji are NOT tofu: the canvas renderer ships only JetBrains Mono
    // (Latin), so without the host OS fallback fonts the CJK "你好世界" run
    // renders as .notdef boxes and the emoji as monochrome/blank. The faces
    // inject LAZILY on the engine's first observed miss (E1) and the pane
    // repaints when they land, so the pixel read POLLS until the glyphs resolve
    // — the same rendered truth, decoupled from the injection round-trip.
    // The CJK+emoji line is the one carrying multi-coloured pixels (emoji), so we
    // FIND that row band by its colour, then assert: (a) CJK glyphs cover the left
    // of the band (real glyphs, not boxes), and (b) the emoji band on the right
    // has many DISTINCT colours (real colour emoji, not mono tofu).
    const readFallbackProbe = (): Promise<{
      found: boolean
      cjkNonBg?: number
      emojiColourful?: number
      distinctEmojiColours?: number
    } | null> =>
      orcaPage.evaluate(
        ({ findSrc, ptyId }) => {
          // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
          const findCanvas = new Function(`return (${findSrc})`)() as (
            id: string
          ) => HTMLCanvasElement | null
          const c = findCanvas(ptyId)
          const ctx = c?.getContext('2d')
          if (!c || !ctx || !c.width) {
            return null
          }
          const W = c.width
          const H = c.height
          const d = ctx.getImageData(0, 0, W, H).data
          const at = (x: number, y: number): [number, number, number] => {
            const i = (y * W + x) * 4
            return [d[i], d[i + 1], d[i + 2]]
          }
          const bg = at(0, 0)
          const isBg = (p: [number, number, number]): boolean =>
            p[0] === bg[0] && p[1] === bg[1] && p[2] === bg[2]
          // A chromatic pixel (channel spread): neither background nor grey/white
          // text — the signature of colour emoji (and the colour ramp/gradient/sixel).
          const isColourful = (p: [number, number, number]): boolean => {
            const max = Math.max(p[0], p[1], p[2])
            const min = Math.min(p[0], p[1], p[2])
            return max - min > 40 && max > 60
          }
          // Find the emoji LINE geometry-independently (the pane's column count — and
          // so where emoji land horizontally — varies with layout): colour emoji form
          // MULTIPLE small chromatic CLUSTERS separated by gaps ("🚀 🔥 ✨ 🦀"),
          // unlike the ramp/gradient rows (one contiguous chromatic run) and the
          // single-hue magenta sixel. Pick the row with ≥3 clusters and the most
          // distinct quantised hues; remember where its first cluster starts (the
          // CJK/Latin run sits left of it).
          const quant = (p: [number, number, number]): number =>
            ((p[0] >> 4) << 8) | ((p[1] >> 4) << 4) | (p[2] >> 4)
          let bestRow = -1
          let bestHues = 0
          let bestClusterX0 = 0
          for (let y = 0; y < H; y++) {
            const hues = new Set<number>()
            let clusters = 0
            let inCluster = false
            let gap = 0
            let firstClusterX0 = -1
            for (let x = 0; x < W; x++) {
              const p = at(x, y)
              if (isColourful(p)) {
                hues.add(quant(p))
                if (!inCluster) {
                  clusters++
                  inCluster = true
                  if (firstClusterX0 < 0) {
                    firstClusterX0 = x
                  }
                }
                gap = 0
              } else if (inCluster) {
                gap++
                if (gap > 8) {
                  inCluster = false
                  gap = 0
                }
              }
            }
            if (clusters >= 3 && hues.size > bestHues) {
              bestHues = hues.size
              bestRow = y
              bestClusterX0 = firstClusterX0
            }
          }
          if (bestRow < 0) {
            return { found: false }
          }
          // Sample a vertical band around the detected row (a glyph spans many rows).
          const y0 = Math.max(0, bestRow - 10)
          const y1 = Math.min(H - 1, bestRow + 10)
          // CJK region: everything LEFT of the first emoji cluster holds
          // "CJK 你好世界  café  Ω≈ç√∫". Count non-bg pixels (real glyphs paint
          // many; blank/sparse .notdef paints far fewer).
          const cjkX1 = Math.max(8, bestClusterX0 - 4)
          let cjkNonBg = 0
          for (let y = y0; y <= y1; y++) {
            for (let x = 0; x < cjkX1; x++) {
              if (!isBg(at(x, y))) {
                cjkNonBg++
              }
            }
          }
          // Emoji region (the clusters and rightward): count chromatic pixels and the
          // DISTINCT quantised colours among them — colour emoji yield many; a mono
          // tofu box or blank slot yields ~0-1.
          const colours = new Set<number>()
          let emojiColourful = 0
          for (let y = y0; y <= y1; y++) {
            for (let x = cjkX1; x < W; x++) {
              const p = at(x, y)
              if (isColourful(p)) {
                emojiColourful++
                colours.add(quant(p))
              }
            }
          }
          return {
            found: true,
            cjkNonBg,
            emojiColourful,
            distinctEmojiColours: colours.size
          }
        },
        { findSrc: CANVAS_BY_PTY, ptyId }
      )

    let fallback: Awaited<ReturnType<typeof readFallbackProbe>> = null
    await expect
      .poll(
        async () => {
          fallback = await readFallbackProbe()
          return Boolean(
            fallback?.found &&
            (fallback.cjkNonBg ?? 0) > 400 &&
            (fallback.emojiColourful ?? 0) > 200 &&
            (fallback.distinctEmojiColours ?? 0) > 8
          )
        },
        {
          timeout: 30_000,
          message: 'CJK + colour-emoji glyphs should resolve once the lazy fallback faces land'
        }
      )
      .toBe(true)

    expect(fallback?.found).toBe(true)
    // CJK "你好世界" rendered real glyphs (not blank, not sparse .notdef boxes).
    expect(fallback?.cjkNonBg ?? 0).toBeGreaterThan(400)
    // Colour emoji rendered: a meaningful count of chromatic pixels spanning many
    // distinct colours (rocket/fire/sparkles/crab are multi-hue). Mono tofu or a
    // blank slot would yield near-zero of each.
    expect(fallback?.emojiColourful ?? 0).toBeGreaterThan(200)
    expect(fallback?.distinctEmojiColours ?? 0).toBeGreaterThan(8)

    const dataUrl = await orcaPage.evaluate(
      ({ findSrc, ptyId }) => {
        // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
        const findCanvas = new Function(`return (${findSrc})`)() as (
          id: string
        ) => HTMLCanvasElement | null
        const c = findCanvas(ptyId)
        return c ? c.toDataURL('image/png') : ''
      },
      { findSrc: CANVAS_BY_PTY, ptyId }
    )
    expect(dataUrl.startsWith('data:image/png;base64,')).toBe(true)
    writeFileSync('/tmp/aterm-showcase.png', Buffer.from(dataUrl.split(',')[1], 'base64'))
  })
})
