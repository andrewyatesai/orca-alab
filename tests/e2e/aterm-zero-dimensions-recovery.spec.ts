// PROVES the "Terminal has zero dimensions" diagnostic does NOT persist on an
// aterm pane once its controller has attached. That banner is a real false alarm:
// the PTY connect path runs a deferred fit() one frame after mount, and if the
// container has not finished layout it can momentarily resolve to 0×0 and surface
// the error toast. Once the aterm controller is live the pane is laid out, so the
// banner must not be stuck on screen. We assert it is NOT visible after the
// controller attaches.

import { test, expect } from './helpers/orca-app'
import { waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveAtermController } from './helpers/aterm-controller'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'

test.describe('aterm zero-dimensions banner does not persist', () => {
  test('no "Terminal has zero dimensions" error once the controller is attached', async ({
    orcaPage
  }) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

    // New terminal tab → its pane is rendered by aterm.
    await orcaPage.getByRole('button', { name: 'New tab' }).click()
    await orcaPage
      .getByRole('menuitem', { name: /New Terminal/i })
      .first()
      .click()

    const canvas = orcaPage.locator('[data-testid="aterm-canvas"]').first()
    await expect(canvas, 'aterm canvas should mount for the new pane').toBeAttached({
      timeout: 20_000
    })

    // Wait for the PTY and the async aterm controller (wasm/font/GPU load): once the
    // controller is live the pane is laid out, so any zero-dimensions toast must be
    // gone (it is the known false alarm this spec guards against).
    await waitForActivePanePtyId(orcaPage)
    await waitForActiveAtermController(orcaPage)

    // The toast renders the error string as plain text in an absolutely-positioned
    // div (TerminalErrorToast). Assert that text is NOT present once attached.
    const zeroDimensionsBanner = orcaPage.getByText(/Terminal has zero dimensions/i)
    await expect(
      zeroDimensionsBanner,
      'the "Terminal has zero dimensions" banner must not persist after the aterm controller attaches'
    ).toHaveCount(0)
  })
})
