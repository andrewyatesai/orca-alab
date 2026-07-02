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

// Locate the ACTIVE pane's canvas by ptyId. The seeded baseline "Terminal 1" tab
// leaves a hidden (display:none, 0×0) background canvas, so a DOM-first/last match
// is unreliable; scope to the laid-out active pane the same way aterm-retina-
// fidelity does.
const findActiveCanvasSrc = `(ptyId) => {
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

test.describe('aterm terminal focus on click', () => {
  test('a real click focuses the terminal and typed input round-trips', async ({ orcaPage }) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

    await orcaPage.getByRole('button', { name: 'New tab' }).click()
    await orcaPage
      .getByRole('menuitem', { name: /New Terminal/i })
      .first()
      .click()

    const ptyId = await waitForActivePanePtyId(orcaPage)
    await waitForActiveAtermController(orcaPage)

    // Wait until the active pane's canvas has a non-zero CSS box (laid out + painted).
    await expect
      .poll(
        async () =>
          orcaPage.evaluate(
            ({ ptyId, findSrc }) => {
              // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
              const find = new Function(`return (${findSrc})`)() as (
                id: string
              ) => HTMLCanvasElement | null
              const c = find(ptyId)
              if (!c) {
                return 0
              }
              const rect = c.getBoundingClientRect()
              return Math.min(rect.width, rect.height)
            },
            { ptyId, findSrc: findActiveCanvasSrc }
          ),
        { timeout: 20_000, message: 'active aterm canvas should be laid out' }
      )
      .toBeGreaterThan(0)

    // Blur everything so the click is unambiguously what focuses the terminal.
    await orcaPage.evaluate(() => (document.activeElement as HTMLElement | null)?.blur?.())

    // A REAL click at the ACTIVE canvas center — the full mouse sequence a user does.
    const center = await orcaPage.evaluate(
      ({ ptyId, findSrc }) => {
        // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
        const find = new Function(`return (${findSrc})`)() as (
          id: string
        ) => HTMLCanvasElement | null
        const c = find(ptyId)
        if (!c) {
          return null
        }
        const r = c.getBoundingClientRect()
        return { x: r.x + r.width / 2, y: r.y + r.height / 2 }
      },
      { ptyId, findSrc: findActiveCanvasSrc }
    )
    expect(center, 'active canvas center should be resolvable').not.toBeNull()
    await orcaPage.mouse.click(center!.x, center!.y)

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
