import type { Page } from '@stablyai/playwright-test'
import { expect } from '@stablyai/playwright-test'

// The aterm pane's controller attaches ASYNCHRONOUSLY — after the wasm engine,
// fallback fonts, and (on the GPU path) the WebGL2 device finish loading. That can
// land well after the PTY is bound, especially under heavy parallel e2e load where
// many Electron instances contend for CPU/GPU. waitForActivePanePtyId only proves
// the PTY exists, so specs that reach into `pane.atermController` right after it can
// race and see "no aterm controller". Poll for the controller to be live first.
export async function waitForActiveAtermController(
  page: Page,
  timeoutMs = 30_000
): Promise<void> {
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
