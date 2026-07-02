import { test, expect } from './helpers/orca-app'
import { execInTerminal, waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveAtermController } from './helpers/aterm-controller'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'
import { countAtermNonBgPixels } from './helpers/aterm-canvas-pixels'
import { writeFileSync } from 'node:fs'

// Proves the aterm in-page renderer (Phase 0): with experimentalAtermRenderer on,
// a terminal pane is painted by aterm-wasm to a <canvas> — xterm.js draws nothing.
// Drives the REAL Electron app: opens a terminal, runs a command, asserts the
// aterm canvas has real glyph pixels, and screenshots it.
test.describe('aterm in-page renderer', () => {
  test('paints a live terminal pane to a canvas (no xterm drawing)', async ({ orcaPage }) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

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
    // Wait for the async aterm controller (wasm/font/GPU load) before driving it —
    // under parallel e2e load it can attach after the PTY binds.
    await waitForActiveAtermController(orcaPage)
    await execInTerminal(
      orcaPage,
      ptyId,
      'printf "\\033[1;32materm\\033[0m renders \\033[1;34mlive\\033[0m in orca: %s\\n" OK'
    )

    // The canvas must contain real rendered (non-background) pixels. The aterm
    // grid canvas may be GPU-owned (webgl2) or CPU-owned (2d) per the auto-policy;
    // countAtermNonBgPixels reads whichever via gl.readPixels or getImageData.
    await expect
      .poll(async () => countAtermNonBgPixels(orcaPage), {
        timeout: 20_000,
        message: 'aterm canvas should have rendered glyph pixels'
      })
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
