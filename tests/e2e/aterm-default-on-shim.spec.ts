import { test, expect } from './helpers/orca-app'
import { waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'
import { countAtermChangedPixelsSince, snapshotAtermCanvas } from './helpers/aterm-canvas-pixels'
import { writeFileSync } from 'node:fs'

// Proves the aterm renderer is a CREDIBLE DEFAULT: the xterm-mirroring shim DOM
// (.xterm / .xterm-screen / .xterm-helper-textarea) that ~29 app consumers key
// off, keyboard input routed through the helper textarea → PTY → canvas, and
// clickable-link detection. Drives the REAL Electron app.
//
// Headless note (ORCA_E2E_HEADLESS): the window is hidden so DOM layout reports
// a 0x0 rect. Keyboard goes to document.activeElement regardless of geometry,
// and link detection is asserted deterministically via the controller probe
// (controller.linkAt) rather than fragile mouse coordinates.

test.describe('aterm renderer as the default', () => {
  test('helper-textarea shim, keyboard→PTY→canvas, and link detection', async ({ orcaPage }) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

    await orcaPage.getByRole('button', { name: 'New tab' }).click()
    await orcaPage
      .getByRole('menuitem', { name: /New Terminal/i })
      .first()
      .click()

    const canvas = orcaPage.locator('[data-testid="aterm-canvas"]').first()
    await expect(canvas, 'aterm canvas should mount').toBeAttached({ timeout: 20_000 })
    await waitForActivePanePtyId(orcaPage)

    // 1) The xterm-mirroring shim DOM exists so focus/paste/IME/clipboard and the
    //    ~29 `.xterm-helper-textarea` / `closest('.xterm')` consumers keep working.
    //    Scope to the ACTIVE aterm pane (the initial tab is an xterm pane now
    //    backgrounded/display:none, so a global query would hit the wrong one).
    const shim = await orcaPage.evaluate(() => {
      const canvasEl = document.querySelector('[data-testid="aterm-canvas"]')
      const xterm = canvasEl?.closest('.xterm') ?? null
      const ta = xterm?.querySelector('.xterm-helper-textarea') as HTMLTextAreaElement | null
      return {
        hasTextarea: !!ta,
        insideXterm: !!ta?.closest('.xterm'),
        insideScreen: !!ta?.closest('.xterm-screen'),
        // Structural focusability: an enabled, tabbable textarea is what makes the
        // app's `.xterm-helper-textarea.focus()` calls land. (Real OS focus can't
        // be asserted under the hidden ORCA_E2E_HEADLESS window.)
        focusable: !!ta && !ta.disabled && ta.tabIndex >= 0
      }
    })
    expect(shim.hasTextarea, 'helper textarea present').toBe(true)
    expect(shim.insideXterm, 'textarea inside .xterm').toBe(true)
    expect(shim.insideScreen, 'textarea inside .xterm-screen').toBe(true)
    expect(shim.focusable, 'textarea is an enabled, tabbable keyboard surface').toBe(true)

    // 2) Keyboard through the shim. Under the input model (mirrors xterm) printable
    //    text flows through the textarea 'input' event (typing/paste/IME), while
    //    non-text keys (Enter) flow through keydown. Drive each accordingly so the
    //    test genuinely exercises the path real typed/pasted text takes:
    //    input → controller onInput → terminal.input → PTY → output mirror → canvas.
    //    (Dispatching events needs no OS focus, so this is deterministic under the
    //    hidden headless window.)
    // Snapshot the canvas, then dispatch the keystrokes; the typed echo + command
    // output must CHANGE the canvas (terminal output isn't monotonic in pixel
    // count — it scrolls/redraws — so assert a pixel DIFF, not a count increase).
    // Snapshot the canvas (GPU swapchain or CPU 2d) BEFORE typing.
    // Snapshot IN-PAGE (the diff runs page-side; only a count crosses IPC). A
    // full-buffer readback is multi-second on a Retina canvas, so polling it would blow
    // the timeout even though the render itself is fine.
    expect(
      await snapshotAtermCanvas(orcaPage, 'type'),
      'should snapshot the canvas before typing'
    ).toBe(true)
    await orcaPage.evaluate(() => {
      const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement | null
      const ta = c
        ?.closest('.xterm')
        ?.querySelector('.xterm-helper-textarea') as HTMLTextAreaElement | null
      if (!ta) {
        throw new Error('no aterm helper textarea')
      }
      // Printable text: set value + dispatch an InputEvent (the path setRangeText
      // paste and typing both produce). data carries the inserted character.
      const type = (text: string): void => {
        for (const ch of text) {
          ta.value = ch
          ta.dispatchEvent(
            new InputEvent('input', { data: ch, inputType: 'insertText', bubbles: true })
          )
        }
      }
      // Non-text key: dispatch keydown (the encoder owns Enter → CR).
      const press = (key: string): void => {
        ta.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))
      }
      type('echo aterm-keyboard-proof-XYZ')
      press('Enter')
    })

    await expect
      .poll(async () => countAtermChangedPixelsSince(orcaPage, 'type'), {
        timeout: 20_000,
        message: 'typing through the shim must change the rendered canvas'
      })
      .toBeGreaterThan(2000)

    // 3) Link detection: feed a line with a URL through the same output path the
    //    PTY uses, then assert (a) the controller detects a URL at the URL's cells
    //    AND (b) the URL's characters actually RENDERED — the canvas changed after
    //    process(), so detection isn't asserting against an unrendered buffer.
    // Baseline the canvas (GPU/CPU), feed the URL, then read it back to confirm
    // the URL glyphs actually painted (a pixel diff), independent of context kind.
    await snapshotAtermCanvas(orcaPage, 'link')
    const link = await orcaPage.evaluate(async () => {
      const managers = (window as unknown as { __paneManagers?: Map<string, unknown> })
        .__paneManagers
      if (!managers) {
        return null
      }
      let controller: {
        process: (d: string) => void
        linkAt: (r: number, c: number) => { url: string; kind: number } | null
      } | null = null
      for (const manager of managers.values()) {
        const m = manager as {
          getActivePane?: () => { atermController?: typeof controller } | null
          getPanes?: () => { atermController?: typeof controller }[]
        }
        const pane = m.getActivePane?.() ?? m.getPanes?.()[0] ?? null
        if (pane?.atermController) {
          controller = pane.atermController
          break
        }
      }
      if (!controller) {
        return null
      }
      const raf = (): Promise<void> =>
        new Promise((resolve) => requestAnimationFrame(() => resolve()))
      // Land the URL on a fresh line at a known column, then let it paint.
      controller.process('\r\nsee https://aterm.example.com/proof here\r\n')
      await raf()
      await raf()
      // Scan the row(s) for a URL hit; the prompt + wrap make the exact row vary,
      // so probe a small window of recent rows × the URL columns.
      for (let row = 0; row < 40; row++) {
        for (let col = 4; col < 36; col++) {
          const hit = controller.linkAt(row, col)
          if (hit && /aterm\.example\.com/.test(hit.url)) {
            return { url: hit.url, kind: hit.kind }
          }
        }
      }
      return { url: '', kind: -1 }
    })
    const renderedPixelDiff = await countAtermChangedPixelsSince(orcaPage, 'link')
    expect(link, 'controller should detect the URL link').not.toBeNull()
    expect(link?.url).toContain('aterm.example.com')
    expect([0, 1]).toContain(link?.kind) // 0 = OSC-8, 1 = detected URL
    // The URL's glyphs must have actually painted (not just detected in the buffer).
    expect(
      renderedPixelDiff,
      'the URL characters should render → canvas pixels change in the URL row'
    ).toBeGreaterThan(0)

    const dataUrl = await orcaPage.evaluate(() => {
      const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement | null
      return c ? c.toDataURL('image/png') : ''
    })
    if (dataUrl.startsWith('data:image/png;base64,')) {
      writeFileSync('/tmp/aterm-default-on.png', Buffer.from(dataUrl.split(',')[1], 'base64'))
    }
  })
})
