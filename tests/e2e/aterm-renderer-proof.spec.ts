import { test, expect } from './helpers/orca-app'
import { execInTerminal, waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'
import { writeFileSync } from 'node:fs'

// Proves the aterm in-page renderer (Phase 0): with experimentalAtermRenderer on,
// a terminal pane is painted by aterm-wasm to a <canvas> — xterm.js draws nothing.
// Drives the REAL Electron app: opens a terminal, runs a command, asserts the
// aterm canvas has real glyph pixels, and screenshots it.
test.describe('aterm in-page renderer', () => {
  test('paints a live terminal pane to a canvas (no xterm drawing)', async ({ orcaPage }) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

    // Turn the aterm renderer on BEFORE the pane that will use it is created.
    await orcaPage.evaluate(() => {
      ;(window as unknown as { __atermRendererEnabled?: boolean }).__atermRendererEnabled = true
    })

    // New terminal tab → its pane is rendered by aterm.
    await orcaPage.getByRole('button', { name: 'New tab' }).click()
    await orcaPage
      .getByRole('menuitem', { name: /New Terminal/i })
      .first()
      .click()

    const canvas = orcaPage.locator('[data-testid="aterm-canvas"]').first()
    // The e2e window is hidden (ORCA_E2E_HEADLESS), so assert it's attached, not
    // "visible"; we verify real rendering via the canvas pixels below.
    await expect(canvas, 'aterm canvas should mount for the new pane').toBeAttached({
      timeout: 20_000
    })

    // Run a colored command; output flows PTY → writeTerminalOutput → aterm canvas.
    const ptyId = await waitForActivePanePtyId(orcaPage)
    await execInTerminal(
      orcaPage,
      ptyId,
      'printf "\\033[1;32materm\\033[0m renders \\033[1;34mlive\\033[0m in orca: %s\\n" OK'
    )

    // The canvas must contain real rendered (non-background) pixels.
    await expect
      .poll(
        async () =>
          orcaPage.evaluate(() => {
            const c = document.querySelector(
              '[data-testid="aterm-canvas"]'
            ) as HTMLCanvasElement | null
            if (!c || !c.width || !c.height) return 0
            const ctx = c.getContext('2d')
            if (!ctx) return 0
            const d = ctx.getImageData(0, 0, c.width, c.height).data
            const bg = [d[0], d[1], d[2]]
            let n = 0
            for (let i = 0; i < d.length; i += 4) {
              if (d[i] !== bg[0] || d[i + 1] !== bg[1] || d[i + 2] !== bg[2]) n++
            }
            return n
          }),
        { timeout: 20_000, message: 'aterm canvas should have rendered glyph pixels' }
      )
      .toBeGreaterThan(500)

    // Read the canvas pixels directly (robust under a hidden window) and save a PNG.
    const dataUrl = await orcaPage.evaluate(() => {
      const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement | null
      return c ? c.toDataURL('image/png') : ''
    })
    expect(dataUrl.startsWith('data:image/png;base64,')).toBe(true)
    writeFileSync('/tmp/aterm-in-orca.png', Buffer.from(dataUrl.split(',')[1], 'base64'))
  })
})
