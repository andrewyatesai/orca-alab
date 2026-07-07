import type { ElectronApplication, Page } from '@stablyai/playwright-test'
import path from 'node:path'
import { test, expect } from './helpers/orca-app'
import { ensureTerminalVisible, waitForActiveWorktree, waitForSessionReady } from './helpers/store'
import { waitForActiveAtermController } from './helpers/aterm-controller'
import {
  execInTerminal,
  waitForActivePanePtyId,
  waitForActiveTerminalManager
} from './helpers/terminal'
import {
  clearTerminalPtyWriteLog,
  installTerminalPtyWriteSpy,
  readTerminalPtyWriteEntries
} from './helpers/terminal-pty-write-spy'

// Asserts the TUI mouse-wheel REPORT contract end-to-end on the aterm engine:
// a notched wheel tick over a mouse-reporting TUI produces SGR wheel reports at
// the PTY — one per notch, scaled by the terminalTuiScrollSensitivity setting.
// Probes engine truth (facade `modes`, real pty:write bytes) rather than
// xterm-era internals (.xterm-screen, _core._renderService, onData — none of
// which exist on the aterm facade; engine reports route via routePtyInput
// straight to pty:write, so the main-process spy is where they are observable).

type WheelReportSample = {
  reportDelta: number
  reports: string[]
}

// ESC built at runtime (not a source control char) so the regex stays free of
// the no-control-regex lint while still matching the real report bytes.
const ESC = String.fromCharCode(27)
const SGR_MOUSE_REPORT = new RegExp(`${ESC}\\[<\\d+;\\d+;\\d+[Mm]`, 'g')
const VISIBLE_TUI_FIXTURE_PATH = path.join(
  process.cwd(),
  'tests/e2e/fixtures/visible-tui-scroll-fixture.cjs'
)

type PaneProbe = {
  container?: ({ dataset?: { ptyId?: string } } & Element) | null
  terminal?: {
    write?: (data: string) => void
    modes?: { mouseTrackingMode?: string }
    rows?: number
    element?: HTMLElement | null
    buffer?: {
      active?: {
        getLine?: (row: number) => { translateToString(trim?: boolean): string } | undefined
      }
    }
  }
}

// The pane bound to ptyId — the identity the test drives. Positional lookups
// (active pane / first canvas) can target a different pane when multiple
// terminal tabs exist.
const PANE_BY_PTY = `(ptyId) => {
  const managers = window.__paneManagers
  for (const mgr of managers?.values() ?? []) {
    for (const pane of mgr.getPanes?.() ?? []) {
      if (pane?.container?.dataset?.ptyId === ptyId) {
        return pane
      }
    }
  }
  return null
}`

async function enableMouseReporting(page: Page, ptyId: string): Promise<void> {
  // Feed the enable sequence through the terminal parser (the same path TUI
  // output takes), then wait for the ENGINE to report tracking active via the
  // facade's live DEC-mode surface.
  await page.evaluate(
    ({ ptyId, findSrc }) => {
      // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
      const find = new Function(`return (${findSrc})`)() as (id: string) => PaneProbe | null
      find(ptyId)?.terminal?.write?.('\x1b[?1003h\x1b[?1006h')
    },
    { ptyId, findSrc: PANE_BY_PTY }
  )
  await expect
    .poll(
      () =>
        page.evaluate(
          ({ ptyId, findSrc }) => {
            // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
            const find = new Function(`return (${findSrc})`)() as (id: string) => PaneProbe | null
            return find(ptyId)?.terminal?.modes?.mouseTrackingMode ?? 'none'
          },
          { ptyId, findSrc: PANE_BY_PTY }
        ),
      { timeout: 10_000, message: 'Mouse reporting mode did not activate' }
    )
    .not.toBe('none')
}

// One notched wheel tick: a LINE-mode wheel event is the canonical discrete
// notch (accumulateWheelLines: lines = deltaY, always "discrete", so the TUI
// sensitivity multiplier applies — exactly how a physical mouse notch scrolls).
async function dispatchNotchedWheelTick(page: Page, ptyId: string): Promise<void> {
  await page.evaluate(
    ({ ptyId, findSrc }) => {
      // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
      const find = new Function(`return (${findSrc})`)() as (id: string) => PaneProbe | null
      const canvas = find(ptyId)?.container?.querySelector('[data-testid="aterm-canvas"]')
      if (!canvas) {
        throw new Error('Active terminal pane unavailable')
      }
      // The aterm mouse-input seam listens on the canvas (capture); wheel-down
      // (positive deltaY) encodes SGR button 65.
      canvas.dispatchEvent(
        new WheelEvent('wheel', { deltaY: 1, deltaMode: 1, bubbles: true, cancelable: true })
      )
    },
    { ptyId, findSrc: PANE_BY_PTY }
  )
}

// Per-tick report capture at the PTY boundary: clear the main-process
// pty:write spy, dispatch one notch, then poll until the report count is
// stable (two consecutive equal non-zero reads) so slow IPC drains never
// split a tick's reports across samples and over-delivery is still caught.
async function probeNotchedWheelTicks(
  page: Page,
  app: ElectronApplication,
  ptyId: string,
  options: { ticks: number; intervalMs: number }
): Promise<WheelReportSample[]> {
  const samples: WheelReportSample[] = []
  for (let tick = 0; tick < options.ticks; tick += 1) {
    await clearTerminalPtyWriteLog(app)
    await dispatchNotchedWheelTick(page, ptyId)
    let reports: string[] = []
    await expect
      .poll(
        async () => {
          const entries = await readTerminalPtyWriteEntries(app)
          const next = entries
            .filter((entry) => entry.id === ptyId)
            .flatMap((entry) => entry.data.match(SGR_MOUSE_REPORT) ?? [])
          const stable = next.length > 0 && next.length === reports.length
          reports = next
          return stable
        },
        { timeout: 5_000, message: `tick ${tick} SGR reports did not arrive/settle` }
      )
      .toBe(true)
    samples.push({ reportDelta: reports.length, reports })
    await page.waitForTimeout(options.intervalMs)
  }
  return samples
}

async function readVisibleTuiOffset(page: Page, ptyId: string): Promise<number | null> {
  return page.evaluate(
    ({ ptyId, findSrc }) => {
      // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
      const find = new Function(`return (${findSrc})`)() as (id: string) => PaneProbe | null
      const terminal = find(ptyId)?.terminal
      if (!terminal) {
        return null
      }
      for (let row = 0; row < (terminal.rows ?? 0); row += 1) {
        const text = terminal.buffer?.active?.getLine?.(row)?.translateToString(true) ?? ''
        const match = /TUI_SCROLL_ROW_(\d+)/.exec(text)
        if (match) {
          return Number(match[1])
        }
      }
      return null
    },
    { ptyId, findSrc: PANE_BY_PTY }
  )
}

async function dispatchTuiWheel(page: Page, ptyId: string, deltaLines: number): Promise<void> {
  await page.evaluate(
    ({ ptyId, findSrc, deltaLines }) => {
      // eslint-disable-next-line @typescript-eslint/no-implied-eval, no-new-func
      const find = new Function(`return (${findSrc})`)() as (id: string) => PaneProbe | null
      const canvas = find(ptyId)?.container?.querySelector('[data-testid="aterm-canvas"]')
      if (!canvas) {
        throw new Error('Active terminal pane unavailable')
      }
      canvas.dispatchEvent(
        new WheelEvent('wheel', {
          deltaY: deltaLines,
          deltaMode: 1,
          bubbles: true,
          cancelable: true
        })
      )
    },
    { ptyId, findSrc: PANE_BY_PTY, deltaLines }
  )
}

async function readyMouseReportingPane(
  page: Page,
  app: ElectronApplication,
  tuiScrollSensitivity: number
): Promise<string> {
  await installTerminalPtyWriteSpy(app)
  await waitForSessionReady(page)
  await waitForActiveWorktree(page)
  await ensureTerminalVisible(page)
  await waitForActiveTerminalManager(page, 30_000)
  await page.evaluate(
    (sensitivity) =>
      window.__store?.getState().updateSettings({ terminalTuiScrollSensitivity: sensitivity }),
    tuiScrollSensitivity
  )
  const ptyId = await waitForActivePanePtyId(page)
  // The aterm controller owns the mouse-input seam; it attaches asynchronously
  // after the PTY binds.
  await waitForActiveAtermController(page)
  await enableMouseReporting(page, ptyId)
  return ptyId
}

test.describe('terminal TUI wheel reports', () => {
  test('notched mouse wheel ticks produce immediate mouse-reporting TUI scroll reports', async ({
    electronApp,
    orcaPage
  }) => {
    const ptyId = await readyMouseReportingPane(orcaPage, electronApp, 1)

    const samples = await probeNotchedWheelTicks(orcaPage, electronApp, ptyId, {
      ticks: 4,
      intervalMs: 60
    })

    expect(
      samples.map((sample) => sample.reportDelta),
      `per-tick SGR mouse reports: ${JSON.stringify(samples)}`
    ).toEqual([1, 1, 1, 1])
    expect(samples.at(-1)?.reports.join('')).toContain('\x1b[<65;')
  })

  test('fullscreen mouse-reporting TUI scroll distance follows wheel magnitude @headful', async ({
    electronApp,
    orcaPage
  }) => {
    await electronApp.evaluate(({ BrowserWindow }) => {
      const win = BrowserWindow.getAllWindows()[0]
      if (!win) {
        throw new Error('No BrowserWindow available')
      }
      if (win.isMinimized()) {
        win.restore()
      }
      win.show()
      win.focus()
      win.setFullScreen(true)
    })
    await expect
      .poll(() =>
        electronApp.evaluate(
          ({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]?.isFullScreen() ?? false
        )
      )
      .toBe(true)
    await orcaPage.waitForTimeout(1200)

    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)
    await ensureTerminalVisible(orcaPage)
    await waitForActiveTerminalManager(orcaPage, 30_000)
    await orcaPage.evaluate(() =>
      window.__store?.getState().updateSettings({ terminalTuiScrollSensitivity: 1 })
    )

    const ptyId = await waitForActivePanePtyId(orcaPage)
    await waitForActiveAtermController(orcaPage)
    await execInTerminal(orcaPage, ptyId, `node ${JSON.stringify(VISIBLE_TUI_FIXTURE_PATH)}`)

    await expect
      .poll(() => readVisibleTuiOffset(orcaPage, ptyId), {
        timeout: 10_000,
        message: 'visible fullscreen TUI did not render numbered rows'
      })
      .toBe(0)

    await dispatchTuiWheel(orcaPage, ptyId, 1)
    await expect
      .poll(() => readVisibleTuiOffset(orcaPage, ptyId), {
        timeout: 5_000,
        message: 'single notched wheel tick did not visibly scroll the TUI'
      })
      .toBe(1)

    // A larger wheel movement scrolls proportionally farther. The fixture caps
    // its offset at page 7 (matching the original xterm-era distance check).
    await dispatchTuiWheel(orcaPage, ptyId, 12)
    await expect
      .poll(() => readVisibleTuiOffset(orcaPage, ptyId), {
        timeout: 5_000,
        message: 'larger wheel movement did not visibly move the TUI farther'
      })
      .toBe(7)
  })

  test('TUI scroll setting scales notched mouse wheel reports', async ({
    electronApp,
    orcaPage
  }) => {
    const ptyId = await readyMouseReportingPane(orcaPage, electronApp, 5)

    // Two gesture paces: sensitivity scaling must hold for slow deliberate
    // ticks and a paced stream alike (aterm's accumulator is time-independent;
    // this guards against a future burst/coalesce layer distorting the setting).
    const slow = await probeNotchedWheelTicks(orcaPage, electronApp, ptyId, {
      ticks: 5,
      intervalMs: 220
    })
    const paced = await probeNotchedWheelTicks(orcaPage, electronApp, ptyId, {
      ticks: 5,
      intervalMs: 80
    })

    expect(
      slow.map((sample) => sample.reportDelta),
      `slow per-tick SGR mouse reports: ${JSON.stringify(slow)}`
    ).toEqual([5, 5, 5, 5, 5])
    expect(
      paced.map((sample) => sample.reportDelta),
      `paced per-tick SGR mouse reports: ${JSON.stringify(paced)}`
    ).toEqual([5, 5, 5, 5, 5])
    expect(
      paced.reduce((sum, sample) => sum + sample.reportDelta, 0),
      `paced SGR mouse reports: ${JSON.stringify(paced)}`
    ).toBe(25)
  })
})
