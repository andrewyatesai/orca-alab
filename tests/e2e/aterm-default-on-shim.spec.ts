import { test, expect } from './helpers/orca-app'
import { waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'
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

type AtermControllerProbe = {
  process: (data: string) => void
  selectionText: () => string
  linkAt: (row: number, col: number) => { url: string; kind: number } | null
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

test.describe('aterm renderer as the default', () => {
  test('helper-textarea shim, keyboard→PTY→canvas, and link detection', async ({ orcaPage }) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

    // Explicit ON wins over the suite-wide opt-out; this exercises the exact code
    // path default users now hit (settings default experimentalAtermRenderer=true).
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
    await orcaPage.evaluate(() => {
      const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement | null
      const ctx = c?.getContext('2d')
      ;(window as unknown as { __beforeType?: Uint8ClampedArray }).__beforeType =
        c && ctx && c.width ? ctx.getImageData(0, 0, c.width, c.height).data.slice() : undefined
      const ta = c
        ?.closest('.xterm')
        ?.querySelector('.xterm-helper-textarea') as HTMLTextAreaElement | null
      if (!ta) throw new Error('no aterm helper textarea')
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
      .poll(
        async () =>
          orcaPage.evaluate(() => {
            const c = document.querySelector(
              '[data-testid="aterm-canvas"]'
            ) as HTMLCanvasElement | null
            const ctx = c?.getContext('2d')
            const before = (window as unknown as { __beforeType?: Uint8ClampedArray }).__beforeType
            if (!c || !ctx || !c.width || !before) return 0
            const d = ctx.getImageData(0, 0, c.width, c.height).data
            if (d.length !== before.length) return d.length // resized → definitely changed
            let changed = 0
            for (let i = 0; i < d.length; i += 4) {
              if (d[i] !== before[i] || d[i + 1] !== before[i + 1] || d[i + 2] !== before[i + 2]) {
                changed++
              }
            }
            return changed
          }),
        { timeout: 20_000, message: 'typing through the shim must change the rendered canvas' }
      )
      .toBeGreaterThan(2000)

    // 3) Link detection: feed a line with a URL through the same output path the
    //    PTY uses, then assert (a) the controller detects a URL at the URL's cells
    //    AND (b) the URL's characters actually RENDERED — the canvas changed after
    //    process(), so detection isn't asserting against an unrendered buffer.
    const link = await orcaPage.evaluate(async () => {
      const managers = (window as unknown as { __paneManagers?: Map<string, unknown> }).__paneManagers
      if (!managers) return null
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
      if (!controller) return null
      const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement | null
      const ctx = c?.getContext('2d')
      const raf = (): Promise<void> =>
        new Promise((resolve) => requestAnimationFrame(() => resolve()))
      // Baseline the canvas, then land the URL on a fresh line at a known column.
      const before =
        c && ctx && c.width ? ctx.getImageData(0, 0, c.width, c.height).data.slice() : null
      controller.process('\r\nsee https://aterm.example.com/proof here\r\n')
      await raf()
      await raf()
      let renderedPixelDiff = 0
      if (c && ctx && c.width && before) {
        const after = ctx.getImageData(0, 0, c.width, c.height).data
        for (let i = 0; i < after.length; i += 4) {
          if (after[i] !== before[i] || after[i + 1] !== before[i + 1] || after[i + 2] !== before[i + 2]) {
            renderedPixelDiff++
          }
        }
      }
      // Scan the row(s) for a URL hit; the prompt + wrap make the exact row vary,
      // so probe a small window of recent rows × the URL columns.
      for (let row = 0; row < 40; row++) {
        for (let col = 4; col < 36; col++) {
          const hit = controller.linkAt(row, col)
          if (hit && /aterm\.example\.com/.test(hit.url)) {
            return { url: hit.url, kind: hit.kind, renderedPixelDiff }
          }
        }
      }
      return { url: '', kind: -1, renderedPixelDiff }
    })
    expect(link, 'controller should detect the URL link').not.toBeNull()
    expect(link?.url).toContain('aterm.example.com')
    expect([0, 1]).toContain(link?.kind) // 0 = OSC-8, 1 = detected URL
    // The URL's glyphs must have actually painted (not just detected in the buffer).
    expect(
      link?.renderedPixelDiff ?? 0,
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
