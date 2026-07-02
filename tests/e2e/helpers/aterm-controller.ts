import type { Page } from '@stablyai/playwright-test'
import { expect } from '@stablyai/playwright-test'

// The aterm pane's controller attaches ASYNCHRONOUSLY — after the wasm engine,
// fallback fonts, and (on the GPU path) the WebGL2 device finish loading. That can
// land well after the PTY is bound, especially under heavy parallel e2e load where
// many Electron instances contend for CPU/GPU. waitForActivePanePtyId only proves
// the PTY exists, so specs that reach into `pane.atermController` right after it can
// race and see "no aterm controller". Poll for the controller to be live first.
export async function waitForActiveAtermController(page: Page, timeoutMs = 30_000): Promise<void> {
  await expect
    .poll(
      async () =>
        page.evaluate(() => {
          const managers = (window as unknown as { __paneManagers?: Map<string, unknown> })
            .__paneManagers
          for (const m of managers?.values() ?? []) {
            const mgr = m as {
              getActivePane?: () => { atermController?: unknown } | null
              getPanes?: () => { atermController?: unknown }[]
            }
            const pane = mgr.getActivePane?.() ?? mgr.getPanes?.()[0] ?? null
            if (pane?.atermController) {
              return true
            }
          }
          return false
        }),
      {
        timeout: timeoutMs,
        message: 'the active pane did not attach an aterm controller (wasm/font/GPU load)'
      }
    )
    .toBe(true)
}

// UNAMBIGUOUS variant: wait for the controller of the pane BOUND TO `ptyId`. The
// active-pane heuristic above can return on a DIFFERENT pane's controller (the
// backgrounded initial tab also attaches one), which under the slow worker-path
// engine build leaves the pane under test still controller-less — its engine then
// misses output-driven work (query replies, OSC-52) or, suspended while hidden,
// never posts the STATE a probe polls.
export async function waitForAtermControllerByPtyId(
  page: Page,
  ptyId: string,
  timeoutMs = 30_000
): Promise<void> {
  await expect
    .poll(
      async () =>
        page.evaluate((id) => {
          const managers = (window as unknown as { __paneManagers?: Map<string, unknown> })
            .__paneManagers
          for (const m of managers?.values() ?? []) {
            const mgr = m as {
              getPanes?: () => { container?: HTMLElement; atermController?: unknown }[]
            }
            for (const pane of mgr.getPanes?.() ?? []) {
              if (pane?.container?.dataset?.ptyId === id && pane.atermController) {
                return true
              }
            }
          }
          return false
        }, ptyId),
      {
        timeout: timeoutMs,
        message: `the pane bound to ${ptyId} did not attach an aterm controller (wasm/font/GPU load)`
      }
    )
    .toBe(true)
}
