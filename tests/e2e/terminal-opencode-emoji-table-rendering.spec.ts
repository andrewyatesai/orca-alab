import { randomUUID } from 'node:crypto'
import { rmSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import type { Page } from '@stablyai/playwright-test'
import { test, expect } from './helpers/orca-app'
import { ensureTerminalVisible, waitForActiveWorktree, waitForSessionReady } from './helpers/store'
import {
  sendToTerminal,
  waitForActivePanePtyId,
  waitForActiveTerminalManager,
  waitForTerminalOutput
} from './helpers/terminal'
import { waitForPtyShellEcho } from './terminal-pty-readiness'
import {
  analyzeRasterCursorCells,
  type TerminalRasterProbeTarget
} from './terminal-cursor-raster-probe'
import { nodeTerminalCommand } from './terminal-node-command'

type TerminalRenderState = {
  coreCursorHidden: boolean | null
  cursorElementCount: number
  cursorVisibleElementCount: number
  cursorBlink: boolean | null
  blinkIntervalDuration: number | null
  cursorClassName: string
  cursorAnimationName: string
  cursorAnimationDuration: string
  rowContainerClassName: string
  xtermClassName: string
  hasWebglCanvas: boolean
  hasComplexScriptOutput: boolean
  renderer: 'dom' | 'webgl'
}

type CursorBlinkSample = {
  elapsedMs: number
  paintedCursorCellCount: number
}

const EMOJI_TABLE_MARKER = 'ORCA_EMOJI_TABLE_RENDER_DONE'

function emojiTableScript(marker: string): string {
  const table = [
    '| Emoji | Name | Age | Occupation | City | Favorite Color | Pet | Hobby |',
    '| --- | --- | ---: | --- | --- | --- | --- | --- |',
    '| 😀 | Alice Johnson | 28 | Engineer | New York | Blue | 🐕 Dog | 🎸 Guitar |',
    '| 😂 | Bob Smith | 34 | Designer | London | Green | 🐱 Cat | 📚 Reading |',
    '| 🥰 | Carol Davis | 22 | Student | Paris | Pink | 🐰 Rabbit | 🎨 Painting |',
    '| 😎 | Dave Wilson | 45 | Architect | Tokyo | Black | 🐢 Turtle | 🏃 Running |',
    '| 🤩 | Eve Martinez | 31 | Writer | Berlin | Purple | 🐦 Bird | ✈️ Traveling |',
    '| 😜 | Frank Brown | 27 | Developer | Sydney | Red | 🐹 Hamster | 🎮 Gaming |',
    '| 🥳 | Grace Lee | 39 | Teacher | Seoul | Yellow | 🐟 Fish | 🌱 Gardening |',
    '| 🤔 | Henry Taylor | 41 | Doctor | Toronto | White | 🐕 Dog | 🍳 Cooking |',
    '| 😴 | Ivy Anderson | 26 | Nurse | Chicago | Orange | 🐱 Cat | 🧘 Yoga |',
    '| 🤗 | Jack Thomas | 33 | Lawyer | Boston | Navy | 🐢 Turtle | 📸 Photography |',
    '| 😈 | Karen White | 29 | Artist | Miami | Teal | 🐹 Hamster | 🧶 Knitting |',
    '| 😮 | Leo Harris | 37 | Pilot | Dubai | Gold | 🐦 Bird | 🚁 Drones |',
    '| 🤠 | Mia Clark | 24 | Barista | Seattle | Coral | 🐰 Rabbit | 🎤 Singing |',
    '| 😍 | Olivia Hall | 30 | Marketer | Austin | Pink | 🐱 Cat | 🏄 Surfing |'
  ].join('\r\n')

  return `
async function writeStdout(chunk) {
  await new Promise((resolve) => {
    process.stdout.write(chunk, resolve)
  })
  if (process.platform === 'win32') {
    await new Promise((resolve) => setTimeout(resolve, 8))
  }
}
await writeStdout('\\x1b[?2026h\\x1b[?25l\\x1b[2J\\x1b[H')
for (const line of ${JSON.stringify(table.split('\r\n'))}) {
  await writeStdout(line + '\\r\\n')
}
await writeStdout('\\x1b[?25h\\x1b[?2026l')
await writeStdout('${marker}\\r\\n')
setTimeout(() => process.exit(0), 50)
`
}

async function readActiveTerminalRenderState(page: Page): Promise<TerminalRenderState> {
  return page.evaluate(() => {
    const state = window.__store?.getState()
    const worktreeId = state?.activeWorktreeId
    const tabId =
      state?.activeTabType === 'terminal'
        ? state.activeTabId
        : worktreeId
          ? (state?.activeTabIdByWorktree?.[worktreeId] ?? null)
          : null
    const manager = tabId ? window.__paneManagers?.get(tabId) : null
    const pane = manager?.getActivePane?.() ?? manager?.getPanes?.()[0] ?? null
    if (!pane) {
      throw new Error('No active terminal pane')
    }
    const renderingDiagnostics = manager
      ?.getRenderingDiagnostics()
      .find((diagnostic) => diagnostic.paneId === pane.id)

    const cursorElements = Array.from(
      pane.container.querySelectorAll<HTMLElement>('.xterm-cursor, .xterm-cursor-layer *')
    )
    const cursorVisibleElementCount = cursorElements.filter((element) => {
      const style = window.getComputedStyle(element)
      const rect = element.getBoundingClientRect()
      return (
        style.display !== 'none' &&
        style.visibility !== 'hidden' &&
        Number(style.opacity || '1') > 0 &&
        rect.width > 0 &&
        rect.height > 0
      )
    }).length

    // Real cursor-hidden state from the aterm engine (cursor_style === 7), not
    // xterm's renderer-internal coreService. Diagnostic-only here; the cursor's
    // actual visibility is proven from the canvas raster below.
    const coreCursorHidden = pane.atermController?.cursorHidden() ?? null
    const cursorElement = pane.container.querySelector<HTMLElement>('.xterm-cursor')
    const cursorStyle = cursorElement ? window.getComputedStyle(cursorElement) : null
    const rowContainer = pane.container.querySelector<HTMLElement>('.xterm-rows')
    const xterm = pane.container.querySelector<HTMLElement>('.xterm')

    return {
      coreCursorHidden,
      cursorElementCount: cursorElements.length,
      cursorVisibleElementCount,
      cursorBlink:
        typeof pane.terminal.options.cursorBlink === 'boolean'
          ? pane.terminal.options.cursorBlink
          : null,
      blinkIntervalDuration:
        typeof pane.terminal.options.blinkIntervalDuration === 'number'
          ? pane.terminal.options.blinkIntervalDuration
          : null,
      cursorClassName: cursorElement?.className ?? '',
      cursorAnimationName: cursorStyle?.animationName ?? '',
      cursorAnimationDuration: cursorStyle?.animationDuration ?? '',
      rowContainerClassName: rowContainer?.className ?? '',
      xtermClassName: xterm?.className ?? '',
      hasWebglCanvas: renderingDiagnostics?.hasWebgl ?? false,
      hasComplexScriptOutput: renderingDiagnostics?.hasComplexScriptOutput ?? false,
      renderer: renderingDiagnostics?.hasWebgl ? 'webgl' : 'dom'
    }
  })
}

async function forceCursorProbeTheme(page: Page): Promise<void> {
  await page.evaluate(() => {
    const state = window.__store?.getState()
    const worktreeId = state?.activeWorktreeId
    const tabId =
      state?.activeTabType === 'terminal'
        ? state.activeTabId
        : worktreeId
          ? (state?.activeTabIdByWorktree?.[worktreeId] ?? null)
          : null
    const manager = tabId ? window.__paneManagers?.get(tabId) : null
    const pane = manager?.getActivePane?.() ?? manager?.getPanes?.()[0] ?? null
    if (!pane) {
      throw new Error('No active terminal pane')
    }
    const controller = pane.atermController
    if (!controller) {
      throw new Error('Active terminal pane has no aterm controller')
    }
    // aterm paints the cursor from the engine theme, not xterm options — route the
    // probe colour through controller.updateTheme. Preserve the resolved bg so the
    // dark table backdrop (and the raster probe's composited-colour match) is intact;
    // an empty palette leaves the already-seeded ANSI colours of the table untouched.
    const bg = (window as unknown as { __resolveAtermThemeBg?: () => [number, number, number] })
      .__resolveAtermThemeBg?.() ?? [17, 19, 24]
    controller.updateTheme({
      fg: 0xd0d0d0,
      bg: (bg[0] << 16) | (bg[1] << 8) | bg[2],
      cursor: 0x23ff45,
      selection: 0x264f78,
      selectionForeground: null,
      palette: []
    })
    // The aterm cursor blink + focus affordance is driven by the helper textarea's
    // focus, not xterm focus; cursorBlink is read live off pane.terminal.options.
    pane.terminal.options.cursorStyle = 'block'
    pane.terminal.options.cursorBlink = true
    const textarea = pane.container.querySelector<HTMLTextAreaElement>('.xterm-helper-textarea')
    textarea?.focus()
  })
}

async function readActiveTerminalRasterTarget(page: Page): Promise<TerminalRasterProbeTarget> {
  return page.evaluate(() => {
    const state = window.__store?.getState()
    const worktreeId = state?.activeWorktreeId
    const tabId =
      state?.activeTabType === 'terminal'
        ? state.activeTabId
        : worktreeId
          ? (state?.activeTabIdByWorktree?.[worktreeId] ?? null)
          : null
    const manager = tabId ? window.__paneManagers?.get(tabId) : null
    const pane = manager?.getActivePane?.() ?? manager?.getPanes?.()[0] ?? null
    if (!pane) {
      throw new Error('No active terminal pane')
    }
    const screen = pane.container.querySelector<HTMLElement>('.xterm-screen')
    if (!screen) {
      throw new Error('Active terminal has no xterm-screen clip surface')
    }
    // aterm owns the pixels on its canvas (no xterm _renderService.dimensions): the
    // .xterm-screen div fills the canvas 1:1, so its rect is the clip and the cell
    // size is the rect divided by the engine grid (atermController.gridSize()).
    const rect = screen.getBoundingClientRect()
    const grid = pane.atermController?.gridSize() ?? {
      cols: pane.terminal.cols,
      rows: pane.terminal.rows
    }
    return {
      clip: { x: rect.x, y: rect.y, width: rect.width, height: rect.height },
      cellWidth: rect.width / grid.cols,
      cellHeight: rect.height / grid.rows,
      rows: grid.rows,
      cols: grid.cols
    }
  })
}

async function sampleCursorBlink(page: Page): Promise<CursorBlinkSample[]> {
  const samples: CursorBlinkSample[] = []
  const target = await readActiveTerminalRasterTarget(page)
  const viewport = page.viewportSize() ?? undefined
  const start = performance.now()
  for (let index = 0; index < 9; index += 1) {
    if (index > 0) {
      await page.waitForTimeout(200)
    }
    const screenshot = await page.screenshot()
    const cells = analyzeRasterCursorCells(Buffer.from(screenshot), target, viewport)
    samples.push({
      elapsedMs: performance.now() - start,
      paintedCursorCellCount: cells.length
    })
  }
  return samples
}

async function enableRiskyTerminalRendererPath(page: Page): Promise<void> {
  await page.evaluate(() => {
    const store = window.__store
    if (!store) {
      throw new Error('window.__store unavailable')
    }
    const state = store.getState()
    store.setState({
      settings: {
        ...state.settings!,
        terminalGpuAcceleration: 'auto',
        theme: 'dark'
      }
    })
    const worktreeId = state.activeWorktreeId
    const tabId =
      state.activeTabType === 'terminal'
        ? state.activeTabId
        : worktreeId
          ? (state.activeTabIdByWorktree?.[worktreeId] ?? null)
          : null
    const manager = tabId ? window.__paneManagers?.get(tabId) : null
    manager?.setTerminalGpuAcceleration('auto')
  })
}

test.describe('OpenCode emoji table terminal rendering', () => {
  test('keeps emoji table output visually sane and restores the cursor', async ({
    orcaPage,
    testRepoPath
  }, testInfo) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)
    await ensureTerminalVisible(orcaPage)
    await waitForActiveTerminalManager(orcaPage, 30_000)
    await enableRiskyTerminalRendererPath(orcaPage)

    const ptyId = await waitForActivePanePtyId(orcaPage)
    await waitForPtyShellEcho(orcaPage, ptyId, 20_000)
    const runId = randomUUID()
    const marker = `${EMOJI_TABLE_MARKER}_${runId}`
    const scriptPath = path.join(testRepoPath, `.orca-opencode-emoji-table-${runId}.mjs`)
    writeFileSync(scriptPath, emojiTableScript(marker))
    try {
      await sendToTerminal(orcaPage, ptyId, `${nodeTerminalCommand([scriptPath])}\r`)
      await waitForTerminalOutput(orcaPage, marker, 10_000)
      await orcaPage.waitForTimeout(250)
      await forceCursorProbeTheme(orcaPage)
      await orcaPage.waitForTimeout(50)

      const renderState = await readActiveTerminalRenderState(orcaPage)
      const blinkSamples = await sampleCursorBlink(orcaPage)

      testInfo.annotations.push({
        type: 'opencode-emoji-table-rendering',
        description: JSON.stringify({ renderState, blinkSamples })
      })

      expect(renderState.hasComplexScriptOutput).toBe(false)
      expect(renderState.renderer).toBe(renderState.hasWebglCanvas ? 'webgl' : 'dom')
      // aterm paints the cursor onto its canvas — there's no .xterm-cursor DOM and no
      // xterm _core coreService to read hidden/blink off (it keeps an UNOPENED xterm
      // shim). The cursor's visibility AND its blink are proven directly from the
      // canvas raster below: some sampled frames paint the cursor cell, others don't.
      expect(blinkSamples.some((sample) => sample.paintedCursorCellCount > 0)).toBe(true)
      expect(blinkSamples.some((sample) => sample.paintedCursorCellCount === 0)).toBe(true)
    } finally {
      rmSync(scriptPath, { force: true })
    }
  })

  test('local real OpenCode demo keeps table rendering and cursor visible', async ({
    orcaPage
  }, testInfo) => {
    test.skip(
      process.env.ORCA_E2E_REAL_OPENCODE !== '1',
      'Set ORCA_E2E_REAL_OPENCODE=1 to exercise the locally installed OpenCode TUI'
    )

    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)
    await ensureTerminalVisible(orcaPage)
    await waitForActiveTerminalManager(orcaPage, 30_000)
    await enableRiskyTerminalRendererPath(orcaPage)

    const ptyId = await waitForActivePanePtyId(orcaPage)
    await sendToTerminal(
      orcaPage,
      ptyId,
      'opencode run --demo --interactive "Give me markdown table dummy data a long table with emojis in it"\r'
    )
    try {
      await waitForTerminalOutput(orcaPage, 'Give me markdown table', 15_000)
      await waitForTerminalOutput(orcaPage, 'Emoji', 60_000)
      await waitForTerminalOutput(orcaPage, 'Alice', 60_000)
      await orcaPage.waitForTimeout(1_500)

      await testInfo.attach('real-opencode-demo-table', {
        body: await orcaPage.screenshot({ fullPage: true }),
        contentType: 'image/png'
      })

      // aterm has no .xterm-cursor DOM / xterm _core to read cursor state from; force
      // the probe cursor colour onto the canvas and prove the cursor is painted (i.e.
      // not hidden) by detecting its raster cells — the aterm-native "cursor visible".
      await forceCursorProbeTheme(orcaPage)
      const cursorTarget = await readActiveTerminalRasterTarget(orcaPage)
      // Why: the probe-cursor recolor repaints on a rAF, so a single screenshot
      // after a fixed 50ms races the repaint and can capture zero probe cells
      // (false fail). Poll the screenshot + raster analysis until the painted
      // cursor cells appear; a genuinely hidden cursor never paints and still
      // fails the guard.
      let paintedCursorCellCount = 0
      await expect
        .poll(
          async () => {
            paintedCursorCellCount = analyzeRasterCursorCells(
              Buffer.from(await orcaPage.screenshot()),
              cursorTarget,
              orcaPage.viewportSize() ?? undefined
            ).length
            return paintedCursorCellCount
          },
          {
            timeout: 10_000,
            message: 'probe cursor was never painted onto the terminal canvas raster'
          }
        )
        .toBeGreaterThan(0)

      const renderState = await readActiveTerminalRenderState(orcaPage)
      testInfo.annotations.push({
        type: 'real-opencode-demo-rendering',
        description: JSON.stringify({ renderState, paintedCursorCellCount })
      })
    } finally {
      await sendToTerminal(orcaPage, ptyId, '\x03').catch(() => undefined)
    }
  })
})
