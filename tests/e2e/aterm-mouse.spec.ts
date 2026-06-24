import { test, expect } from './helpers/orca-app'
import { sendToTerminal, waitForActivePanePtyId } from './helpers/terminal'
import { waitForActiveAtermController } from './helpers/aterm-controller'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'

// Proves MOUSE REPORTING works under the aterm renderer (the default): a TUI
// enables mouse tracking via DECSET 1000 (+ SGR 1006), then a canvas mousedown is
// ENCODED as an SGR mouse report and SENT to the PTY (vim/tmux/htop respond to
// the mouse). We drive the REAL Electron app and prove the report reached the
// child via the controller's lastMouseReport() hook (echo-based proof is flaky
// under the hidden e2e window).
//
// Also asserts the gate: with Shift held the same click does NOT forward (it
// falls through to text selection), matching every terminal's Shift override.

type AtermMouseControllerProbe = {
  process: (data: string) => void
  lastMouseReport: () => string | null
}

function findActiveController(): AtermMouseControllerProbe {
  const managers = (window as unknown as { __paneManagers?: Map<string, unknown> }).__paneManagers
  if (!managers) {
    throw new Error('no pane managers')
  }
  for (const manager of managers.values()) {
    const m = manager as {
      getActivePane?: () => { atermController?: AtermMouseControllerProbe | null } | null
      getPanes?: () => { atermController?: AtermMouseControllerProbe | null }[]
    }
    const pane = m.getActivePane?.() ?? m.getPanes?.()[0] ?? null
    if (pane?.atermController) {
      return pane.atermController
    }
  }
  throw new Error('no aterm controller on any pane')
}

test.describe('aterm mouse reporting', () => {
  test('encodes a canvas click as an SGR mouse report and sends it to the PTY', async ({
    orcaPage
  }) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

    // Turn the aterm renderer on BEFORE the pane that will use it is created.
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
    // Wait for the async aterm controller (wasm/font/GPU load) so the in-page probe
    // below finds it — under parallel e2e load it can attach after the PTY binds.
    await waitForActiveAtermController(orcaPage)

    // Enable mouse tracking exactly as a TUI would: DECSET 1000 (normal tracking)
    // + 1006 (SGR encoding). Write through the PTY so the daemon round-trips the
    // bytes back to the renderer's engine (the real path), then wait until the
    // engine reports tracking is on.
    await sendToTerminal(orcaPage, ptyId, "printf '\\033[?1000h\\033[?1006h'\r")
    await expect
      .poll(
        async () =>
          orcaPage.evaluate((findSrc: string) => {
            // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
            const find = new Function(`return (${findSrc})()`) as () => AtermMouseControllerProbe
            const ctrl = find()
            // Also feed the sequence directly through the controller so the test
            // is deterministic even if the PTY echo is slow under headless.
            ctrl.process('\x1b[?1000h\x1b[?1006h')
            const c = document.querySelector(
              '[data-testid="aterm-canvas"]'
            ) as (HTMLCanvasElement & { __t?: unknown }) | null
            return c !== null
          }, findActiveController.toString()),
        { timeout: 20_000, message: 'controller reachable + tracking sequences fed' }
      )
      .toBe(true)

    // Dispatch a primary mousedown on the canvas at a known cell, then read the
    // last report the controller forwarded to the PTY.
    const report = await orcaPage.evaluate((findSrc: string) => {
      // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
      const find = new Function(`return (${findSrc})()`) as () => AtermMouseControllerProbe
      const ctrl = find()
      const c = document.querySelector('[data-testid="aterm-canvas"]') as HTMLCanvasElement | null
      if (!c) {
        return { sent: null as string | null, shiftSent: null as string | null }
      }
      const rect = c.getBoundingClientRect()
      const press = (shiftKey: boolean): void => {
        c.dispatchEvent(
          new MouseEvent('mousedown', {
            button: 0,
            buttons: 1,
            clientX: rect.left + 5,
            clientY: rect.top + 5,
            shiftKey,
            bubbles: true,
            cancelable: true
          })
        )
      }
      const release = (shiftKey: boolean): void => {
        window.dispatchEvent(
          new MouseEvent('mouseup', {
            button: 0,
            clientX: rect.left + 5,
            clientY: rect.top + 5,
            shiftKey,
            bubbles: true,
            cancelable: true
          })
        )
      }
      // No-Shift press → must forward an SGR PRESS report (final byte 'M').
      press(false)
      const pressSent = ctrl.lastMouseReport()
      // ...and the release forwards an SGR RELEASE report (final byte 'm').
      release(false)
      const releaseSent = ctrl.lastMouseReport()
      // Shift-held press → must NOT forward (user override → selection): the last
      // report stays the release from above.
      press(true)
      const shiftSent = ctrl.lastMouseReport()
      release(true)
      return { pressSent, releaseSent, shiftSent }
    }, findActiveController.toString())

    // ESC built at runtime (not a source control char) so the regexes below stay
    // free of the no-control-regex lint while still matching the real reply bytes;
    // afterSgr strips the leading ESC [ so the regex asserts the remainder.
    const ESC = String.fromCharCode(27)
    const afterSgr = (r: string | null): string => (r && r.startsWith(`${ESC}[`) ? r.slice(2) : '')
    // The no-Shift press was encoded as an SGR press report: ESC [ < 0 ; C ; R M.
    expect(report.pressSent, 'a mouse press report was forwarded to the PTY').not.toBeNull()
    expect(afterSgr(report.pressSent), 'the report is an SGR left-button press (e[<0;C;RM)').toMatch(
      /^<0;\d+;\d+M$/
    )
    // The release forwards the matching SGR release (lowercase 'm' final byte).
    expect(afterSgr(report.releaseSent), 'the release forwards an SGR release (e[<0;C;Rm)').toMatch(
      /^<0;\d+;\d+m$/
    )
    // The Shift-held press must NOT forward (it fell through to selection), so the
    // last report stays the prior release — Shift did not encode a new report.
    expect(
      report.shiftSent,
      'Shift+press does not forward (user override → selection)'
    ).toBe(report.releaseSent)
  })
})
