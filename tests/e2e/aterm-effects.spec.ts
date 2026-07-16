import { test, expect } from './helpers/orca-app'
import type { Page } from '@stablyai/playwright-test'
import { waitForActivePanePtyId } from './helpers/terminal'
import { waitForAtermControllerByPtyId } from './helpers/aterm-controller'
import { waitForActiveWorktree, waitForSessionReady } from './helpers/store'
import {
  countAtermChangedPixelsInBand,
  countAtermChangedPixelsSince,
  snapshotAtermCanvas,
  snapshotAtermCanvasBand
} from './helpers/aterm-frame-diff'

// PROVES the aterm effects integration end-to-end on the REAL app:
//  1) settings plumbing — enabling sparkle words + cursor glow via the settings
//     store reaches the live engine (the same keys the Terminal Engine panel writes);
//  2) animation drive — after typing a lexicon word ("orca": the orca class; the
//     builtin emphasis class ships EMPTY — the earlier 'ultrathink' seed was removed
//     from the engine lexicon — so an orca-class word is the honest probe), canvas
//     pixels near the cursor/word row keep changing across frames with NO new input;
//  3) idle-to-zero — the animation settles: frames become byte-identical across a
//     quiet window (no permanent rAF work);
//  4) byte-stability off — disabling the effects restores byte-identical frames.
//
// Drives the in-process CPU path (worker off by harness default, GPU disabled
// below) for deterministic 2d pixel reads; the effects surface is shared by all
// three paths (same engine crate). Everything is scoped to the pane bound to the
// active ptyId — with two terminal tabs open, the document-first canvas belongs
// to the BACKGROUNDED initial pane.

// A quiet window with zero changed pixels = settled/byte-stable. 350ms spans
// several would-be animation frames at any refresh rate.
const QUIET_WINDOW_MS = 350

async function settleToByteStableFrames(page: Page, key: string, ptyId: string): Promise<void> {
  await expect
    .poll(
      async () => {
        if (!(await snapshotAtermCanvas(page, key, ptyId))) {
          return -1
        }
        await page.waitForTimeout(QUIET_WINDOW_MS)
        return countAtermChangedPixelsSince(page, key, ptyId)
      },
      { timeout: 30_000, message: `frames should settle to byte-identical (${key})` }
    )
    .toBe(0)
}

// Find the pane bound to `ptyId` and type `text` through its helper textarea —
// the same input path a real keystroke takes (textarea input event → inputSink →
// PTY → echo → engine). With multiple tabs, typing must NOT target the
// document-first textarea (it belongs to the backgrounded initial pane).
async function typeIntoPane(page: Page, ptyId: string, text: string): Promise<void> {
  await page.evaluate(
    ({ ptyId, text }) => {
      const managers = (
        window as unknown as {
          __paneManagers?: Map<string, { getPanes?: () => { container?: HTMLElement | null }[] }>
        }
      ).__paneManagers
      for (const mgr of managers?.values() ?? []) {
        for (const pane of mgr.getPanes?.() ?? []) {
          if (pane?.container?.dataset?.ptyId === ptyId) {
            const ta = pane.container.querySelector(
              '.xterm-helper-textarea'
            ) as HTMLTextAreaElement | null
            if (!ta) {
              throw new Error('no helper textarea on the target pane')
            }
            ta.value = text
            ta.dispatchEvent(new InputEvent('input', { data: text, bubbles: true }))
            return
          }
        }
      }
      throw new Error('no pane bound to the target ptyId')
    },
    { ptyId, text }
  )
}

test.describe('aterm effects (sparkle words + cursor glow)', () => {
  test('animate near the word/cursor, settle to zero, and disable byte-stably', async ({
    orcaPage
  }) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)

    // CPU path for deterministic 2d reads (the GPU/worker paths share the engine).
    await orcaPage.evaluate(() => {
      ;(window as unknown as { __atermGpuDisabled?: boolean }).__atermGpuDisabled = true
    })

    // Enable the effects VIA SETTINGS (the same store keys the Terminal Engine
    // panel writes) BEFORE the pane opens, so the pane-open engine-settings apply
    // is what turns them on. Cursor blink off so "settled" means byte-identical.
    await orcaPage.evaluate(async () => {
      await window.__store?.getState().updateSettings({
        terminalEffectsSparkleWords: true,
        terminalEffectsCursorGlow: true,
        terminalCursorBlink: false
      })
    })

    await orcaPage.getByRole('button', { name: 'New tab' }).click()
    await orcaPage
      .getByRole('menuitem', { name: /New Terminal/i })
      .first()
      .click()
    const canvas = orcaPage.locator('[data-testid="aterm-canvas"]').first()
    await expect(canvas, 'an aterm canvas should mount').toBeAttached({ timeout: 20_000 })
    const ptyId = await waitForActivePanePtyId(orcaPage)
    await waitForAtermControllerByPtyId(orcaPage, ptyId)

    // Let the prompt render + any startup output settle first, so later pixel
    // changes are attributable to the effects, not shell startup.
    await settleToByteStableFrames(orcaPage, 'prompt-idle', ptyId)

    // Type a lexicon word through the real input path (helper textarea → PTY →
    // echo → engine). "orca" is in the builtin orca class (water splash); the
    // cursor jump also wakes the glow comet.
    await typeIntoPane(orcaPage, ptyId, 'echo orca')

    // Wait for the echoed text to land on the canvas at all.
    await expect
      .poll(async () => countAtermChangedPixelsSince(orcaPage, 'prompt-idle', ptyId), {
        timeout: 15_000,
        message: 'the typed text should render'
      })
      .toBeGreaterThan(10)

    // Compute the device-pixel band around the cursor row (± 2 rows: the splash
    // droplets arc above the word; the glow hugs the cursor row).
    const band = await orcaPage.evaluate((ptyId) => {
      const managers = (
        window as unknown as {
          __paneManagers?: Map<
            string,
            {
              getPanes?: () => {
                container?: HTMLElement | null
                atermController?: {
                  cursorY: () => number
                  cellSizeCss: () => { width: number; height: number }
                } | null
              }[]
            }
          >
        }
      ).__paneManagers
      for (const mgr of managers?.values() ?? []) {
        for (const pane of mgr.getPanes?.() ?? []) {
          if (pane?.container?.dataset?.ptyId === ptyId && pane.atermController) {
            const dpr = window.devicePixelRatio || 1
            const cellDeviceH = pane.atermController.cellSizeCss().height * dpr
            // Glow is ON here, so the frame carries window-space chrome and the
            // grid starts at (pad, pad+head). Read the vertical offset back from
            // the canvas's negative marginTop so the band stays cursor-anchored.
            const canvas = pane.container.querySelector(
              '[data-testid="aterm-canvas"]'
            ) as HTMLCanvasElement | null
            const chromeTopDevice = Math.round(
              -Number.parseFloat(canvas?.style.marginTop || '0') * dpr || 0
            )
            const y = Math.max(
              0,
              Math.round((pane.atermController.cursorY() - 2) * cellDeviceH + chromeTopDevice)
            )
            return { y, h: Math.round(cellDeviceH * 5) }
          }
        }
      }
      throw new Error('no aterm controller for the target pane')
    }, ptyId)

    // ANIMATION: with NO new input, successive frames near the word/cursor keep
    // changing. Require at least two distinct changing intervals so a single
    // late repaint can't pass as an animation.
    let changingIntervals = 0
    for (let i = 0; i < 12 && changingIntervals < 2; i++) {
      expect(await snapshotAtermCanvasBand(orcaPage, `anim-${i}`, band, ptyId)).toBe(true)
      await orcaPage.waitForTimeout(120)
      if ((await countAtermChangedPixelsInBand(orcaPage, `anim-${i}`, ptyId)) > 10) {
        changingIntervals++
      }
    }
    expect(
      changingIntervals,
      'pixels near the word/cursor should keep changing across frames (animation)'
    ).toBeGreaterThanOrEqual(2)

    // IDLE-TO-ZERO: the effects self-terminate; frames become byte-identical
    // across a quiet window and STAY that way (no permanent rAF work).
    await settleToByteStableFrames(orcaPage, 'settled', ptyId)
    expect(await snapshotAtermCanvas(orcaPage, 'settled-hold', ptyId)).toBe(true)
    await orcaPage.waitForTimeout(QUIET_WINDOW_MS)
    expect(
      await countAtermChangedPixelsSince(orcaPage, 'settled-hold', ptyId),
      'settled frames should stay byte-identical'
    ).toBe(0)

    // DISABLE: live settings change reaches the open pane (reapplyEngineSettings);
    // sparkle-off drops decoration state and restores byte-stable frames.
    await orcaPage.evaluate(async () => {
      await window.__store?.getState().updateSettings({
        terminalEffectsSparkleWords: false,
        terminalEffectsCursorGlow: false
      })
    })
    await settleToByteStableFrames(orcaPage, 'disabled', ptyId)
    expect(await snapshotAtermCanvas(orcaPage, 'disabled-hold', ptyId)).toBe(true)
    await orcaPage.waitForTimeout(QUIET_WINDOW_MS)
    expect(
      await countAtermChangedPixelsSince(orcaPage, 'disabled-hold', ptyId),
      'frames with effects disabled should be byte-identical'
    ).toBe(0)
  })
})
