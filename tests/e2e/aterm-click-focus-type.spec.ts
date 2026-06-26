import { test, expect } from './helpers/orca-app'
import { waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveAtermController } from './helpers/aterm-controller'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'

// Regression for the focus-on-click wiring. The aterm grid <canvas> is NOT a
// focusable element, so a REAL click's mousedown default moves focus to <body>,
// blurring the helper textarea — leaving the terminal unfocused and unable to
// receive keystrokes (cursor drawn hollow). No prior spec caught this because none
// performed a real click and typed: they fed input via controller.process() and
// drove focus programmatically. This spec exercises the actual user path
// (click → focus → type → echo) so the wiring can never silently regress again.
test.describe('aterm terminal focus on click', () => {
  test('a real click focuses the terminal and typed input round-trips', async ({ orcaPage }) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

    await orcaPage.evaluate(() => {
      ;(window as unknown as { __atermRendererEnabled?: boolean }).__atermRendererEnabled = true
    })

    await orcaPage.getByRole('button', { name: 'New tab' }).click()
    await orcaPage
      .getByRole('menuitem', { name: /New Terminal/i })
      .first()
      .click()

    const canvas = orcaPage.locator('[data-testid="aterm-canvas"]').first()
    await expect(canvas, 'aterm canvas should mount for the new pane').toBeAttached({
      timeout: 20_000
    })
    await waitForActivePanePtyId(orcaPage)
    await waitForActiveAtermController(orcaPage)

    // Blur everything so the click is unambiguously what focuses the terminal.
    await orcaPage.evaluate(() => (document.activeElement as HTMLElement | null)?.blur?.())

    // A REAL click on the terminal canvas — the full mouse sequence a user performs.
    await canvas.click()

    // Focus MUST land on the aterm helper textarea, not <body>. Pre-fix this was
    // <body> because the canvas mousedown default stole focus away from the textarea.
    const activeClass = await orcaPage.evaluate(
      () => document.activeElement?.className ?? document.activeElement?.tagName ?? 'null'
    )
    expect(activeClass, 'click should focus the helper textarea, not body').toContain(
      'xterm-helper-textarea'
    )

    // And the focused terminal must accept typed input end-to-end: the shell echoes
    // the line back, so the marker appears in the off-screen a11y mirror (body text).
    await orcaPage.keyboard.type('echo orcaFocusWorks42')
    await orcaPage.keyboard.press('Enter')
    await expect
      .poll(
        async () => orcaPage.evaluate(() => document.body.innerText.includes('orcaFocusWorks42')),
        { timeout: 15_000, message: 'typed command should reach the PTY and echo back' }
      )
      .toBe(true)
  })
})
