import { randomUUID } from 'node:crypto'
import type { Page } from '@stablyai/playwright-test'
import { expect } from '@stablyai/playwright-test'
import { sendToTerminal } from './helpers/terminal'
import {
  getTerminalContentForPtyId,
  waitForPtyPaneMounted,
  waitForPtyShellEcho
} from './terminal-pty-readiness'
import { nodeTerminalCommand } from './terminal-node-command'
import { buildFreshShellProbeInputSequence } from './terminal-probe-input-sequence'
import { stripSerializedControlSequences } from './terminal-serialized-text'

type TerminalColumnProbeWindow = Window & {
  __store?: {
    getState: () => {
      activeTabId?: string | null
      activeTabIdByWorktree?: Record<string, string | undefined>
      activeTabType?: string | null
      activeWorktreeId?: string | null
    }
  }
  __paneManagers?: Map<
    string,
    {
      getActivePane?: () => { terminal?: { cols?: number } } | null
      getPanes?: () => { terminal?: { cols?: number } }[]
    }
  >
}

async function readActiveRenderedTerminalColumns(page: Page): Promise<number> {
  return page.evaluate(() => {
    const { __paneManagers: paneManagers, __store: store } = window as TerminalColumnProbeWindow
    const state = store?.getState()
    const worktreeId = state?.activeWorktreeId
    const tabId =
      state?.activeTabType === 'terminal'
        ? state.activeTabId
        : worktreeId
          ? (state?.activeTabIdByWorktree?.[worktreeId] ?? null)
          : null
    const manager = tabId ? paneManagers?.get(tabId) : null
    const pane = manager?.getActivePane?.() ?? manager?.getPanes?.()[0] ?? null
    return pane?.terminal?.cols ?? 0
  })
}

export async function waitForRenderedTerminalColumnsAtMost(
  page: Page,
  maxCols: number,
  timeoutMs = 10_000
): Promise<number> {
  let observedCols = 0
  await expect
    .poll(
      async () => {
        observedCols = await readActiveRenderedTerminalColumns(page)
        return observedCols > 0 ? observedCols : maxCols + 1
      },
      {
        timeout: timeoutMs,
        message: `rendered terminal columns did not settle at or below ${maxCols}`
      }
    )
    .toBeLessThanOrEqual(maxCols)
  return observedCols
}

// Why: after a viewport resize + sidebar toggle the pane width reflows in stages,
// so a single read can catch a transient (or sticky) mid-reflow column count
// (e.g. 33 while the right sidebar still occupies width) that is "at most maxCols"
// but not the settled width. A table sized to / reflowed at that squeezed width
// wraps ~4x taller and splits cell names across rows, breaking downstream
// assertions. Wait until the column count holds steady across consecutive reads
// AND lands within [minCols, maxCols] so callers can require the real wide
// (sidebar-closed) width, not just any value under the cap.
export async function waitForStableRenderedTerminalColumnsAtMost(
  page: Page,
  maxCols: number,
  { timeoutMs = 15_000, stableForMs = 750, pollIntervalMs = 150, minCols = 1 } = {}
): Promise<number> {
  const deadline = Date.now() + timeoutMs
  let lastCols = -1
  let stableSince = Date.now()
  let observedCols = 0
  while (Date.now() < deadline) {
    observedCols = await readActiveRenderedTerminalColumns(page)
    if (observedCols > 0 && observedCols === lastCols) {
      if (
        Date.now() - stableSince >= stableForMs &&
        observedCols <= maxCols &&
        observedCols >= minCols
      ) {
        return observedCols
      }
    } else {
      lastCols = observedCols
      stableSince = Date.now()
    }
    await page.waitForTimeout(pollIntervalMs)
  }
  throw new Error(
    `rendered terminal columns did not stabilize within [${minCols}, ${maxCols}] within ${timeoutMs}ms; last observed ${observedCols}`
  )
}

export async function waitForPtyColumnsAtMost(
  page: Page,
  ptyId: string,
  maxCols: number,
  timeoutMs = 30_000
): Promise<number> {
  const deadline = Date.now() + timeoutMs
  await waitForPtyPaneMounted(page, ptyId, Math.min(10_000, timeoutMs))
  await waitForPtyShellEcho(page, ptyId, Math.min(15_000, Math.max(0, deadline - Date.now())))
  let markerObserved = false
  let lastObservedCols: number | null = null
  let lastMarker = ''
  let lastTerminalTail = ''
  while (Date.now() < deadline) {
    const marker = `ORCA_PTY_COLUMNS_${randomUUID()}`
    lastMarker = marker
    for (const input of buildFreshShellProbeInputSequence(
      `${nodeTerminalCommand([
        '-e',
        `console.log('${marker}:' + (process.stdout.columns || 0))`
      ])}\r`
    )) {
      await sendToTerminal(page, ptyId, input)
    }
    const probeDeadline = Date.now() + Math.min(5_000, Math.max(0, deadline - Date.now()))
    while (Date.now() < probeDeadline) {
      const content = await getTerminalContentForPtyId(page, ptyId, 30_000)
      lastTerminalTail = content
      // Why: a narrow PTY wraps "<marker>:<cols>" across rows, and serialize
      // splits it with cursor-move/clear-line controls. Strip those so the
      // value still parses regardless of the column width being probed.
      const match = stripSerializedControlSequences(content).match(new RegExp(`${marker}:(\\d+)`))
      const observedCols = Number(match?.[1] ?? 0)
      if (observedCols > 0) {
        markerObserved = true
        lastObservedCols = observedCols
        break
      }
      await page.waitForTimeout(100)
    }
    if (lastObservedCols !== null && lastObservedCols <= maxCols) {
      return lastObservedCols
    }
    const retryDelayMs = Math.min(250, Math.max(0, deadline - Date.now()))
    if (retryDelayMs > 0) {
      await page.waitForTimeout(retryDelayMs)
    }
  }
  lastTerminalTail = await getTerminalContentForPtyId(page, ptyId, 30_000)
  const finalState = {
    lastMarker,
    markerObserved,
    lastObservedCols,
    maxCols,
    terminalTail: lastTerminalTail.slice(-4_000)
  }
  if (!markerObserved) {
    throw new Error(
      `PTY column probe never observed a marker within ${timeoutMs}ms; final state ${JSON.stringify(
        finalState
      )}`
    )
  }
  throw new Error(
    `PTY columns stayed above ${maxCols}; last observed ${lastObservedCols}; final state ${JSON.stringify(
      finalState
    )}`
  )
}
