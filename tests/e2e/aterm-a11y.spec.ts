import { test, expect } from './helpers/orca-app'
import { execInTerminal, waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveAtermController } from './helpers/aterm-controller'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'

// PROVES GAP-2 accessibility: the aterm pane renders to an opaque <canvas>, so
// screen readers see nothing unless the visible grid is mirrored into an ARIA
// live region. Drives the REAL Electron app with the aterm renderer on, runs a
// command, then asserts the off-screen role="log"/aria-live region contains the
// rendered output text — exactly what assistive tech would announce.

const MARKER = 'aterm-a11y-marker-ZQX'

test.describe('aterm screen-reader accessibility', () => {
  test('the ARIA live region mirrors rendered terminal output', async ({ orcaPage }) => {
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
    await expect(canvas, 'aterm canvas should mount').toBeAttached({ timeout: 20_000 })
    const ptyId = await waitForActivePanePtyId(orcaPage)
    // Wait for the async aterm controller (wasm/font/GPU load) so the live-region
    // mirror is wired — under parallel e2e load it can attach after the PTY binds.
    await waitForActiveAtermController(orcaPage)

    // At least one aterm live region must exist with screen-reader semantics:
    // role="log", aria-live="polite", and NOT display:none (screen readers ignore
    // display:none — it must be off-screen instead). There can be more than one
    // aterm pane (a backgrounded initial pane + the active one), so assert the
    // semantics hold for EVERY live region the aterm renderer mounts.
    const regions = await orcaPage.evaluate(() =>
      Array.from(document.querySelectorAll('.xterm [role="log"]')).map((el) => ({
        ariaLive: el.getAttribute('aria-live'),
        ariaAtomic: el.getAttribute('aria-atomic'),
        hidden: getComputedStyle(el as HTMLElement).display === 'none'
      }))
    )
    expect(regions.length, 'an ARIA live region (role="log") must exist for the aterm pane').toBeGreaterThan(0)
    for (const r of regions) {
      expect(r.ariaLive, 'the live region must announce updates politely').toBe('polite')
      expect(r.ariaAtomic, 'aria-atomic=false so only changed text is announced').toBe('false')
      expect(r.hidden, 'the live region must be off-screen, NOT display:none').toBe(false)
    }

    // Run a command whose output carries a unique marker, then assert SOME aterm
    // live region mirrors it — i.e. the rendered grid was mirrored for screen
    // readers. We scan all live regions because the active pane (whose mirror gets
    // the output) isn't necessarily the document-order-first aterm canvas.
    await execInTerminal(orcaPage, ptyId, `printf "${MARKER}\\n"`)

    await expect
      .poll(
        async () =>
          orcaPage.evaluate(() =>
            Array.from(document.querySelectorAll('.xterm [role="log"]')).some((el) =>
              (el.textContent ?? '').includes('aterm-a11y-marker-ZQX')
            )
          ),
        {
          timeout: 20_000,
          message: 'an aterm ARIA live region should mirror the rendered terminal output'
        }
      )
      .toBe(true)

    // eslint-disable-next-line no-console
    console.log('[aterm-a11y] PASS — live region mirrored the terminal output')
  })
})
