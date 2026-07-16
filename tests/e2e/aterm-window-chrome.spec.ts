import { test, expect } from './helpers/orca-app'
import type { Page } from '@stablyai/playwright-test'
import { execInTerminal, getTerminalLogicalText, waitForActivePanePtyId } from './helpers/terminal'
import { waitForAtermControllerByPtyId } from './helpers/aterm-controller'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'

// PROVES the window-space effects chrome end-to-end on the PRODUCTION render path
// (the worker-ON gate runs this with ORCA_E2E_ATERM_WORKER=1, so the shared render
// worker stays enabled):
//  1) enabling the fire cursor glow via the settings store grants the pane chrome —
//     the worker STATE reports chromePadPx/chromeHeadPx > 0 and the pane canvas box
//     is pulled up-left by exactly -(pad/dpr) / -((pad+head)/dpr), so the grid stays
//     put and only the chrome overhangs;
//  2) the grid stays intact under chrome: real output lands on the correct logical
//     row (logical-text read, not pixels);
//  3) chrome follows the config: switching the style to 'water' keeps it (EVERY
//     glow style emits past the grid), and disabling the glow returns the frame to
//     the byte-identical 0/0 default with 0px margins.

type ChromeProbe = {
  chromePadPx: number
  chromeHeadPx: number
  marginLeftPx: number
  marginTopPx: number
  dpr: number
} | null

// Read the worker STATE's chrome fields (the sibling worker specs' probe) plus the
// TARGET pane's canvas margins, scoped by ptyId — the document-first canvas belongs
// to the backgrounded bootstrap pane.
async function readChromeProbe(page: Page, ptyId: string): Promise<ChromeProbe> {
  return page.evaluate((ptyId) => {
    const managers = (
      window as unknown as {
        __paneManagers?: Map<
          string,
          {
            getPanes?: () => {
              container?: {
                dataset?: { ptyId?: string }
                querySelector: (s: string) => Element | null
              }
            }[]
          }
        >
      }
    ).__paneManagers
    let canvas: HTMLCanvasElement | null = null
    for (const mgr of managers?.values() ?? []) {
      for (const pane of mgr.getPanes?.() ?? []) {
        if (pane?.container?.dataset?.ptyId === ptyId) {
          canvas = pane.container.querySelector(
            '[data-testid="aterm-canvas"]'
          ) as HTMLCanvasElement | null
        }
      }
    }
    const state = (
      window as unknown as {
        __atermWorkerRenderState?: {
          chromePadPx?: number
          chromeHeadPx?: number
        }
      }
    ).__atermWorkerRenderState
    if (
      !canvas ||
      typeof state?.chromePadPx !== 'number' ||
      typeof state.chromeHeadPx !== 'number'
    ) {
      return null
    }
    return {
      chromePadPx: state.chromePadPx,
      chromeHeadPx: state.chromeHeadPx,
      // Number.parseFloat('') is NaN — an unwritten margin reads as 0.
      marginLeftPx: Number.parseFloat(canvas.style.marginLeft || '0') || 0,
      marginTopPx: Number.parseFloat(canvas.style.marginTop || '0') || 0,
      dpr: window.devicePixelRatio || 1
    }
  }, ptyId)
}

// Margins are device-px offsets serialized through CSS px; allow float noise.
const PX_TOLERANCE = 0.02

function marginsMatchChrome(p: NonNullable<ChromeProbe>): boolean {
  return (
    Math.abs(p.marginLeftPx - -(p.chromePadPx / p.dpr)) <= PX_TOLERANCE &&
    Math.abs(p.marginTopPx - -((p.chromePadPx + p.chromeHeadPx) / p.dpr)) <= PX_TOLERANCE
  )
}

test.describe('aterm window-space effects chrome (worker path)', () => {
  test('fire glow grants chrome, the grid stays intact, and disabling restores 0/0', async ({
    orcaPage
  }) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

    // Force the worker render path on BEFORE the pane (mirrors the sibling worker
    // specs): the default harness forces it off, and this spec's STATE probe +
    // margin assertions are the worker seam's. The worker-ON gate leaves the flag
    // unset, so this line is a no-op there.
    await orcaPage.evaluate(() => {
      ;(window as unknown as { __atermWorkerRender?: boolean }).__atermWorkerRender = true
    })

    // Enable the fire glow VIA SETTINGS (the same keys the Terminal Engine panel
    // writes) BEFORE the pane opens, so the pane-open engine-settings apply grants
    // the chrome. Fire is the style that motivated the head band (flames rise).
    await orcaPage.evaluate(async () => {
      await window.__store?.getState().updateSettings({
        terminalEffectsCursorGlow: true,
        terminalEffectsCursorGlowStyle: 'fire'
      })
    })

    await orcaPage.getByRole('button', { name: 'New tab' }).click()
    await orcaPage
      .getByRole('menuitem', { name: /New Terminal/i })
      .first()
      .click()
    const canvas = orcaPage.locator('[data-testid="aterm-canvas"]').first()
    await expect(canvas, 'an aterm canvas should mount').toBeAttached({
      timeout: 20_000
    })
    const ptyId = await waitForActivePanePtyId(orcaPage)
    await waitForAtermControllerByPtyId(orcaPage, ptyId)

    // 1) The pane STATE reports chrome and the canvas box carries the matching
    // negative margins (grid pinned in place, chrome overhanging).
    let probe: ChromeProbe = null
    await expect
      .poll(
        async () => {
          probe = await readChromeProbe(orcaPage, ptyId)
          return probe !== null && probe.chromePadPx > 0 && probe.chromeHeadPx > 0
        },
        {
          timeout: 30_000,
          message: 'the worker STATE should report fire-glow chrome (pad/head > 0)'
        }
      )
      .toBe(true)
    await expect
      .poll(
        async () => {
          probe = await readChromeProbe(orcaPage, ptyId)
          return probe !== null && probe.chromePadPx > 0 && marginsMatchChrome(probe)
        },
        {
          timeout: 15_000,
          message: `canvas margins should equal -(pad/dpr)/-((pad+head)/dpr); last probe: ${JSON.stringify(probe)}`
        }
      )
      .toBe(true)
    expect(probe!.marginLeftPx, 'marginLeft must be negative under chrome').toBeLessThan(0)
    expect(probe!.marginTopPx, 'marginTop must be negative under chrome').toBeLessThan(0)

    // 2) Grid integrity under chrome: real output lands on the correct logical row
    // (buffer text read, render-path independent — no pixel positions).
    await execInTerminal(orcaPage, ptyId, 'echo hi')
    await expect
      .poll(async () => getTerminalLogicalText(orcaPage), {
        timeout: 15_000,
        message: 'the echoed output should land on its own logical row'
      })
      .toMatch(/^hi$/m)

    // 3a) Style switch to 'water': chrome persists (the gate covers ALL glow styles).
    await orcaPage.evaluate(async () => {
      await window.__store?.getState().updateSettings({ terminalEffectsCursorGlowStyle: 'water' })
    })
    // Give the live reapply a beat to round-trip through the worker, then assert
    // the chrome held (the config seam applies glow + chrome atomically, so no
    // transient 0/0 is expected in between).
    await orcaPage.waitForTimeout(250)
    await expect
      .poll(
        async () => {
          probe = await readChromeProbe(orcaPage, ptyId)
          return probe !== null && probe.chromePadPx > 0 && marginsMatchChrome(probe)
        },
        {
          timeout: 15_000,
          message: 'water glow must keep the window chrome (all-styles gate)'
        }
      )
      .toBe(true)

    // 3b) Disabling the glow entirely returns the byte-identical 0/0 frame and
    // restores the explicit 0px margins.
    await orcaPage.evaluate(async () => {
      await window.__store?.getState().updateSettings({ terminalEffectsCursorGlow: false })
    })
    await expect
      .poll(
        async () => {
          probe = await readChromeProbe(orcaPage, ptyId)
          return (
            probe !== null &&
            probe.chromePadPx === 0 &&
            probe.chromeHeadPx === 0 &&
            probe.marginLeftPx === 0 &&
            probe.marginTopPx === 0
          )
        },
        {
          timeout: 15_000,
          message: `disabling the glow should reset chrome + margins to 0; last probe: ${JSON.stringify(probe)}`
        }
      )
      .toBe(true)
  })
})
