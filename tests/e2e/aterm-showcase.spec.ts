import { test, expect } from './helpers/orca-app'
import { waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'
import { writeFileSync } from 'node:fs'

// A holistic "experience" capture: renders a rich real-world frame through the
// aterm engine (true color, 256-color ramp, box-drawing table, CJK + emoji,
// programming ligatures, a Sixel image) and saves a PNG. Captured on the CPU
// path (clean toDataURL in headless); GPU output is pixel-identical (parity is
// proven separately in aterm-webgl.spec.ts), so this is representative of the
// live GPU experience. Asserts the frame has substantial rendered content.

type Probe = { process: (d: string) => void }
function findController(): Probe {
  const managers = (window as unknown as { __paneManagers?: Map<string, unknown> }).__paneManagers
  for (const m of managers?.values() ?? []) {
    const mgr = m as {
      getActivePane?: () => { atermController?: Probe | null } | null
      getPanes?: () => { atermController?: Probe | null }[]
    }
    const pane = mgr.getActivePane?.() ?? mgr.getPanes?.()[0] ?? null
    if (pane?.atermController) {
      return pane.atermController
    }
  }
  throw new Error('no aterm controller')
}

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
      ;(window as unknown as { __atermRendererEnabled?: boolean }).__atermRendererEnabled = true
      ;(window as unknown as { __atermGpuDisabled?: boolean }).__atermGpuDisabled = true
    })
    await orcaPage.getByRole('button', { name: 'New tab' }).click()
    await orcaPage
      .getByRole('menuitem', { name: /New Terminal/i })
      .first()
      .click()
    const canvas = orcaPage.locator('[data-testid="aterm-canvas"]').first()
    await expect(canvas).toBeAttached({ timeout: 20_000 })
    await waitForActivePanePtyId(orcaPage)

    const payload = [
      '\x1b[2J\x1b[H',
      '\x1b[1;38;5;81m aterm\x1b[0m \x1b[2min orca — GPU terminal\x1b[0m\r\n\r\n',
      // 256-color ramp
      Array.from({ length: 32 }, (_, i) => `\x1b[48;5;${16 + i * 6}m \x1b[0m`).join(''),
      '\r\n',
      // true-color gradient
      Array.from({ length: 32 }, (_, i) => `\x1b[48;2;${i * 8};${128};${255 - i * 8}m \x1b[0m`).join(
        ''
      ),
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
      (args: { findSrc: string; payload: string }) => {
        // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
        const find = new Function(`return (${args.findSrc})()`) as () => {
          process: (d: string) => void
        }
        find().process(args.payload)
      },
      { findSrc: findController.toString(), payload }
    )

    // Substantial rendered content (not a blank/near-blank frame).
    const nonBg = await expect
      .poll(
        async () =>
          orcaPage.evaluate(() => {
            const c = document.querySelector(
              '[data-testid="aterm-canvas"]'
            ) as HTMLCanvasElement | null
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
          }),
        { timeout: 20_000, message: 'showcase frame should have rich content' }
      )
      .toBeGreaterThan(5000)
    void nonBg

    // CJK + emoji are NOT tofu: the canvas renderer ships only JetBrains Mono
    // (Latin), so without the host OS fallback fonts injected over IPC the CJK
    // "你好世界" run renders as .notdef boxes and the emoji as monochrome/blank.
    // The CJK+emoji line is the one carrying multi-coloured pixels (emoji), so we
    // FIND that row band by its colour, then assert: (a) CJK glyphs cover the left
    // of the band (real glyphs, not boxes), and (b) the emoji band on the right
    // has many DISTINCT colours (real colour emoji, not mono tofu).
    const fallback = await orcaPage.evaluate(() => {
      const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement | null
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
      // Find the emoji LINE. The emoji band is SPARSE (mostly background, a few
      // colourful glyph clusters) and MULTI-HUE, unlike the dense, full-width
      // colour ramp/gradient rows and the single-hue magenta sixel block. So for
      // each row scan the right ~55% (where the emoji sit, past "CJK 你好世界 …"):
      // qualify a row by colourful pixels spanning MANY distinct quantised hues,
      // and pick the row with the most distinct hues.
      const emojiX0 = Math.floor(W * 0.45)
      const quant = (p: [number, number, number]): number =>
        ((p[0] >> 4) << 8) | ((p[1] >> 4) << 4) | (p[2] >> 4)
      let bestRow = -1
      let bestHues = 0
      for (let y = 0; y < H; y++) {
        const hues = new Set<number>()
        for (let x = emojiX0; x < W; x++) {
          const p = at(x, y)
          if (isColourful(p)) {
            hues.add(quant(p))
          }
        }
        // The dense ramp/gradient rows live in the LEFT 40% and have ~0 colour in
        // the right 55%, so they never win here; the magenta sixel is one hue.
        if (hues.size > bestHues) {
          bestHues = hues.size
          bestRow = y
        }
      }
      if (bestRow < 0) {
        return { found: false }
      }
      // Sample a vertical band around the detected row (a glyph spans many rows).
      const y0 = Math.max(0, bestRow - 10)
      const y1 = Math.min(H - 1, bestRow + 10)
      // CJK region: the left ~40% holds "CJK 你好世界  café …". Count non-bg
      // pixels (real glyphs paint many; blank/sparse .notdef paints far fewer).
      const cjkX1 = Math.floor(W * 0.4)
      let cjkNonBg = 0
      for (let y = y0; y <= y1; y++) {
        for (let x = 0; x < cjkX1; x++) {
          if (!isBg(at(x, y))) {
            cjkNonBg++
          }
        }
      }
      // Emoji region (right of the CJK/Latin run): count chromatic pixels and the
      // DISTINCT quantised colours among them — colour emoji yield many; a mono
      // tofu box or blank slot yields ~0-1.
      const colours = new Set<number>()
      let emojiColourful = 0
      for (let y = y0; y <= y1; y++) {
        for (let x = emojiX0; x < W; x++) {
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
    })

    expect(fallback?.found).toBe(true)
    // CJK "你好世界" rendered real glyphs (not blank, not sparse .notdef boxes).
    expect(fallback?.cjkNonBg ?? 0).toBeGreaterThan(400)
    // Colour emoji rendered: a meaningful count of chromatic pixels spanning many
    // distinct colours (rocket/fire/sparkles/crab are multi-hue). Mono tofu or a
    // blank slot would yield near-zero of each.
    expect(fallback?.emojiColourful ?? 0).toBeGreaterThan(200)
    expect(fallback?.distinctEmojiColours ?? 0).toBeGreaterThan(8)

    const dataUrl = await orcaPage.evaluate(() => {
      const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement | null
      return c ? c.toDataURL('image/png') : ''
    })
    expect(dataUrl.startsWith('data:image/png;base64,')).toBe(true)
    writeFileSync('/tmp/aterm-showcase.png', Buffer.from(dataUrl.split(',')[1], 'base64'))
  })
})
