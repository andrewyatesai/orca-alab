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
    if (pane?.atermController) return pane.atermController
  }
  throw new Error('no aterm controller')
}

// A solid 24x12 magenta Sixel block (deterministic, ASCII-only payload).
function sixelBlock(): string {
  const ESC = '\x1b'
  let s = `${ESC}Pq#0;2;100;0;100#0`
  for (let band = 0; band < 2; band++) {
    s += '#0' + '~'.repeat(24) + '$-'
  }
  return s + `${ESC}\\`
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
      Array.from({ length: 32 }, (_, i) => `\x1b[48;5;${16 + i * 6}m \x1b[0m`).join('') + '\r\n',
      // true-color gradient
      Array.from({ length: 32 }, (_, i) => `\x1b[48;2;${i * 8};${128};${255 - i * 8}m \x1b[0m`).join(
        ''
      ) + '\r\n\r\n',
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
            if (!c || !ctx || !c.width) return 0
            const d = ctx.getImageData(0, 0, c.width, c.height).data
            const bg = [d[0], d[1], d[2]]
            let n = 0
            for (let i = 0; i < d.length; i += 4) {
              if (d[i] !== bg[0] || d[i + 1] !== bg[1] || d[i + 2] !== bg[2]) n++
            }
            return n
          }),
        { timeout: 20_000, message: 'showcase frame should have rich content' }
      )
      .toBeGreaterThan(5000)
    void nonBg

    const dataUrl = await orcaPage.evaluate(() => {
      const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement | null
      return c ? c.toDataURL('image/png') : ''
    })
    expect(dataUrl.startsWith('data:image/png;base64,')).toBe(true)
    writeFileSync('/tmp/aterm-showcase.png', Buffer.from(dataUrl.split(',')[1], 'base64'))
  })
})
