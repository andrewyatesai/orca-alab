import { test, expect } from './helpers/orca-app'
import { execInTerminal, waitForActivePanePtyId } from './helpers/terminal'
import { waitForAtermControllerByPtyId } from './helpers/aterm-controller'
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

    await orcaPage.getByRole('button', { name: 'New tab' }).click()
    await orcaPage
      .getByRole('menuitem', { name: /New Terminal/i })
      .first()
      .click()

    const canvas = orcaPage.locator('[data-testid="aterm-canvas"]').first()
    await expect(canvas, 'aterm canvas should mount').toBeAttached({ timeout: 20_000 })
    const ptyId = await waitForActivePanePtyId(orcaPage)
    // Wait for THIS pane's aterm controller (by ptyId; wasm/font/GPU load) so the
    // live-region mirror is wired — under parallel e2e load it can attach after the
    // PTY binds (and the backgrounded initial pane's controller can attach first).
    await waitForAtermControllerByPtyId(orcaPage, ptyId)

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
    expect(
      regions.length,
      'an ARIA live region (role="log") must exist for the aterm pane'
    ).toBeGreaterThan(0)
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

  // Proves the live region accumulates the REAL terminal text, in order, and that
  // NEW output APPENDS (the overlap-diff) rather than re-announcing the whole grid.
  // Two commands print ordered, distinctive multi-line output; the off-screen
  // role="log" region must end up containing every line, each as its own child
  // <div>, with the first command's lines BEFORE the second's — exactly the
  // accessible scrollback a screen reader navigates.
  test('the ARIA live region accumulates multi-line output in order (append-only)', async ({
    orcaPage
  }) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

    await orcaPage.getByRole('button', { name: 'New tab' }).click()
    await orcaPage
      .getByRole('menuitem', { name: /New Terminal/i })
      .first()
      .click()

    const canvas = orcaPage.locator('[data-testid="aterm-canvas"]').first()
    await expect(canvas, 'aterm canvas should mount').toBeAttached({ timeout: 20_000 })
    const ptyId = await waitForActivePanePtyId(orcaPage)
    await waitForAtermControllerByPtyId(orcaPage, ptyId)

    // First command: three ordered, distinctive lines.
    const A = ['A1-aterm-log-line-alpha', 'A2-aterm-log-line-bravo', 'A3-aterm-log-line-charlie']
    await execInTerminal(orcaPage, ptyId, `printf "${A.join('\\n')}\\n"`)
    await expect
      .poll(
        async () =>
          orcaPage.evaluate(
            (needle) =>
              Array.from(document.querySelectorAll('.xterm [role="log"]')).some((el) =>
                (el.textContent ?? '').includes(needle)
              ),
            A[2]
          ),
        { timeout: 20_000, message: 'first command output should reach the live region' }
      )
      .toBe(true)

    // Second command: three more ordered lines printed AFTER the first.
    const B = ['B1-aterm-log-line-delta', 'B2-aterm-log-line-echo', 'B3-aterm-log-line-foxtrot']
    await execInTerminal(orcaPage, ptyId, `printf "${B.join('\\n')}\\n"`)
    await expect
      .poll(
        async () =>
          orcaPage.evaluate(
            (needle) =>
              Array.from(document.querySelectorAll('.xterm [role="log"]')).some((el) =>
                (el.textContent ?? '').includes(needle)
              ),
            B[2]
          ),
        { timeout: 20_000, message: 'second command output should append to the live region' }
      )
      .toBe(true)

    // Inspect the live region that holds our output: assert (1) it contains ALL six
    // lines, (2) in the printed order, (3) each as a discrete child <div> (the
    // structure that makes the log reviewable line-by-line by assistive tech), and
    // (4) the first command's lines come BEFORE the second's (append-only, the
    // overlap-diff didn't drop or reorder history).
    const all = [...A, ...B]
    const report = await orcaPage.evaluate((markers) => {
      const regions = Array.from(document.querySelectorAll('.xterm [role="log"]'))
      const region = regions.find((el) => (el.textContent ?? '').includes(markers[0]))
      if (!region) {
        return null
      }
      const text = region.textContent ?? ''
      // Each line must be present.
      const present = markers.map((m) => text.includes(m))
      // Order: the index of each marker in the accumulated text must be ascending.
      const positions = markers.map((m) => text.indexOf(m))
      let ordered = true
      for (let i = 1; i < positions.length; i++) {
        if (positions[i] <= positions[i - 1]) {
          ordered = false
        }
      }
      // Each marker should land in its OWN discrete child div (line-granular review).
      const childTexts = Array.from(region.children).map((c) => c.textContent ?? '')
      const eachMarkerHasOwnDiv = markers.every((m) => childTexts.some((ct) => ct.includes(m)))
      const childTagsAllDiv = Array.from(region.children).every(
        (c) => c.tagName.toLowerCase() === 'div'
      )
      return {
        present,
        ordered,
        eachMarkerHasOwnDiv,
        childTagsAllDiv,
        childCount: region.children.length
      }
    }, all)

    expect(report, 'a live region containing our output should exist').not.toBeNull()
    expect(report!.present.every(Boolean), 'every printed line must be mirrored').toBe(true)
    expect(report!.ordered, 'lines must accumulate in printed order (append-only)').toBe(true)
    expect(report!.eachMarkerHasOwnDiv, 'each line is a discrete child node').toBe(true)
    expect(report!.childTagsAllDiv, 'live-region children are <div> line nodes').toBe(true)
    expect(report!.childCount, 'the log accumulated multiple line nodes').toBeGreaterThanOrEqual(6)
  })
})
