/**
 * E2E repro for terminal output bursts from many background tabs.
 *
 * This is a scaled-down version of the user report: several terminal tabs are
 * mounted, inactive tabs emit large output bursts, and the focused tab must
 * still render a foreground marker while the background output drains through
 * the shared scheduler instead of direct xterm writes.
 */

import type { Page } from '@stablyai/playwright-test'
import { test, expect } from './helpers/orca-app'
import {
  ensureTerminalVisible,
  getActiveTabId,
  waitForActiveWorktree,
  waitForSessionReady
} from './helpers/store'
import { getTerminalContent, waitForActiveTerminalManager } from './helpers/terminal'

type SchedulerDebugSnapshot = {
  backgroundEnqueueCount: number
  deferredForegroundEnqueueCount: number
  foregroundWriteCount: number
  backgroundWriteCount: number
  deferredForegroundWriteCount: number
  flushWriteCount: number
  scheduledDrainCount: number
  drainWrites: number[]
}

type SchedulerDebugWindow = Window & {
  __terminalOutputSchedulerDebug?: {
    reset: () => void
    snapshot: () => SchedulerDebugSnapshot
  }
}

const SORTABLE_TAB = '[data-testid="sortable-tab"]'
const TAB_COUNT = 5

function tabLocator(page: Page, tabId: string) {
  return page.locator(`${SORTABLE_TAB}[data-tab-id="${tabId}"]`).first()
}

async function countRenderedTabs(page: Page): Promise<number> {
  return page.locator(SORTABLE_TAB).count()
}

async function getDomActiveTabId(page: Page): Promise<string | null> {
  return page.evaluate((selector) => {
    const match = document.querySelector(`${selector}[data-active="true"]`)
    return match?.getAttribute('data-tab-id') ?? null
  }, SORTABLE_TAB)
}

function nodeConsoleCommand(expression: string): string {
  return `node -e "console.log(${expression})"`
}

function nodeScriptCommand(script: string): string {
  return `node -e "${script}"`
}

async function createTerminalTab(page: Page): Promise<string> {
  const tabsBefore = await countRenderedTabs(page)
  const activeBefore = await getActiveTabId(page)

  const newTabButton = page.getByRole('button', { name: 'New tab' })
  const newTerminalMenuItem = page.getByRole('menuitem', { name: /New Terminal/i }).first()

  // Why: this spec creates tabs in a tight loop while the previous terminal is
  // still mounting. Two races made a plain open-then-click flake: (1) the "+"
  // trigger click can land mid-re-render so the Radix dropdown never opens and
  // the menuitem never appears; (2) the dropdown can auto-close between a
  // visibility check and the menuitem click (onCloseAutoFocus re-focuses the
  // freshly-mounted terminal), so the click then waits out its full timeout.
  // Drive open -> select inside one poll whose success signal is the new tab
  // actually rendering: each attempt re-opens the menu if it closed and
  // re-clicks the menuitem, so a transient close just retries. force:true
  // bypasses the actionability "stable" check that the animated terminal
  // surface keeps invalidating in hidden-window Electron; short per-call
  // timeouts keep a missed open from stalling the whole attempt.
  await expect
    .poll(
      async () => {
        // Stop driving the menu the moment the new tab exists so a late re-open
        // can't create a second tab; the count is the real success signal.
        const current = await countRenderedTabs(page)
        if (current > tabsBefore) {
          return current
        }
        // One action per attempt: open the menu if it's closed, otherwise
        // select "New Terminal". Never doing both in the same tick avoids both
        // the "click before the dropdown opened" race and re-selecting after a
        // pending create, which would spawn a duplicate tab.
        const target = (await newTerminalMenuItem.isVisible()) ? newTerminalMenuItem : newTabButton
        await target.click({ force: true, timeout: 1_000 }).catch(() => {})
        return countRenderedTabs(page)
      },
      {
        timeout: 15_000,
        message: 'Clicking "+" → New Terminal did not render a new tab in the tab bar'
      }
    )
    .toBe(tabsBefore + 1)

  // Why: success can land while the menu is still open (a re-open attempt fired
  // just before the create rendered). Close it so it can't intercept the next
  // createTerminalTab loop or the tab clicks that follow.
  if (await newTerminalMenuItem.isVisible()) {
    await page.keyboard.press('Escape')
    await expect(newTerminalMenuItem).toBeHidden({ timeout: 5_000 })
  }

  let tabId: string | null = null
  await expect
    .poll(
      async () => {
        tabId = await getActiveTabId(page)
        return Boolean(tabId && tabId !== activeBefore)
      },
      {
        timeout: 5_000,
        message: 'New Terminal did not become the active tab'
      }
    )
    .toBe(true)

  if (!tabId) {
    throw new Error('createTerminalTab: active tab id was unavailable after creating terminal')
  }
  return tabId
}

async function waitForTabPtyId(page: Page, tabId: string): Promise<string> {
  let ptyId: string | null = null
  await expect
    .poll(
      async () => {
        ptyId = await page.evaluate((targetTabId) => {
          const manager = window.__paneManagers?.get(targetTabId)
          const pane = manager?.getPanes?.()[0] ?? null
          return pane?.container?.dataset?.ptyId ?? null
        }, tabId)
        return ptyId
      },
      {
        timeout: 15_000,
        message: `Terminal tab ${tabId} did not receive a PTY binding`
      }
    )
    .not.toBeNull()

  if (!ptyId) {
    throw new Error(`waitForTabPtyId: tab ${tabId} has no PTY id`)
  }
  return ptyId
}

async function resetSchedulerDebug(page: Page): Promise<void> {
  await page.evaluate(() => {
    const debug = (window as SchedulerDebugWindow).__terminalOutputSchedulerDebug
    if (!debug) {
      throw new Error('terminal output scheduler debug API is unavailable')
    }
    debug.reset()
  })
}

async function getSchedulerDebug(page: Page): Promise<SchedulerDebugSnapshot> {
  return page.evaluate(() => {
    const debug = (window as SchedulerDebugWindow).__terminalOutputSchedulerDebug
    if (!debug) {
      throw new Error('terminal output scheduler debug API is unavailable')
    }
    return debug.snapshot()
  })
}

async function sendPtyCommands(
  page: Page,
  commands: { ptyId: string; command: string }[]
): Promise<void> {
  await page.evaluate((items) => {
    for (const item of items) {
      window.api.pty.write(item.ptyId, `${item.command}\r`)
    }
  }, commands)
}

async function mainSnapshotContains(page: Page, ptyId: string, text: string): Promise<boolean> {
  return page.evaluate(
    async ({ targetPtyId, expectedText }) => {
      const snapshot = await window.api.pty.getMainBufferSnapshot(targetPtyId, {
        scrollbackRows: 200
      })
      return snapshot?.data.includes(expectedText) ?? false
    },
    { targetPtyId: ptyId, expectedText: text }
  )
}

// Why: a freshly spawned shell's line editor (zsh ZLE) is not yet input-ready
// the instant the pane binds a ptyId. Writing immediately drops the first typed
// bytes (e.g. "node" arrives as "de"), so the real command silently fails with
// "command not found" and never produces output. Round-trip a sentinel — Ctrl-C
// + Ctrl-U first clears any partial line (matching discoverActivePtyId) — and
// wait for its echoed OUTPUT before sending the command under test.
async function waitForShellInputReady(page: Page, ptyId: string): Promise<void> {
  const sentinel = `__SHELL_READY_${Date.now()}_${Math.random().toString(36).slice(2)}__`
  await expect
    .poll(
      async () => {
        await page.evaluate(
          ({ targetPtyId, token }) => {
            window.api.pty.write(targetPtyId, `\x03\x15echo ${token}\r`)
          },
          { targetPtyId: ptyId, token: sentinel }
        )
        return mainSnapshotContains(page, ptyId, `\n${sentinel}`)
      },
      {
        timeout: 20_000,
        message: 'Shell never became input-ready (sentinel echo not observed)'
      }
    )
    .toBe(true)
}

test.describe('Terminal output scheduler', () => {
  test('background tab output bursts use the shared drain while the active tab renders', async ({
    orcaPage
  }) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)
    await ensureTerminalVisible(orcaPage)
    await waitForActiveTerminalManager(orcaPage, 30_000)

    const firstTabId = await getActiveTabId(orcaPage)
    if (!firstTabId) {
      throw new Error('Expected an initial terminal tab')
    }

    const tabIds = [firstTabId]
    const ptyIdsByTabId: Record<string, string> = {
      [firstTabId]: await waitForTabPtyId(orcaPage, firstTabId)
    }

    while (tabIds.length < TAB_COUNT) {
      const tabId = await createTerminalTab(orcaPage)
      await waitForActiveTerminalManager(orcaPage, 30_000)
      tabIds.push(tabId)
      ptyIdsByTabId[tabId] = await waitForTabPtyId(orcaPage, tabId)
    }

    await tabLocator(orcaPage, firstTabId).click()
    await expect
      .poll(() => getDomActiveTabId(orcaPage), {
        timeout: 5_000,
        message: 'First terminal tab did not become active before the burst repro'
      })
      .toBe(firstTabId)

    await resetSchedulerDebug(orcaPage)

    const runId = Date.now()
    const foregroundMarker = `FG_SCHED_${runId}`
    // Why: the marker is appended AFTER the burst payload so it survives
    // getTerminalContent's tail-only truncation (charLimit defaults to 4000).
    // A leading marker would be evicted by the 50000-char x-burst.
    const backgroundCommands = tabIds.slice(1).map((tabId, index) => ({
      ptyId: ptyIdsByTabId[tabId],
      marker: `BG_SCHED_${runId}_${index}`,
      command: nodeConsoleCommand(`'x'.repeat(50000) + ':BG_SCHED_${runId}_${index}'`)
    }))

    await sendPtyCommands(
      orcaPage,
      backgroundCommands.map(({ ptyId, command }) => ({ ptyId, command }))
    )
    await sendPtyCommands(orcaPage, [
      {
        ptyId: ptyIdsByTabId[firstTabId],
        command: nodeConsoleCommand(`'${foregroundMarker}'`)
      }
    ])

    await expect
      .poll(async () => (await getTerminalContent(orcaPage)).includes(foregroundMarker), {
        timeout: 5_000,
        message: 'Active terminal did not render foreground output during background bursts'
      })
      .toBe(true)

    await expect
      .poll(
        async () => {
          const debug = await getSchedulerDebug(orcaPage)
          if (debug.backgroundEnqueueCount >= backgroundCommands.length) {
            return true
          }
          const snapshots = await Promise.all(
            backgroundCommands.map(({ ptyId, marker }) =>
              mainSnapshotContains(orcaPage, ptyId, marker)
            )
          )
          return snapshots.every(Boolean)
        },
        {
          timeout: 30_000,
          message: 'Background PTY output was not retained by the scheduler or main snapshot'
        }
      )
      .toBe(true)

    await expect
      .poll(
        async () => {
          const debug = await getSchedulerDebug(orcaPage)
          return debug.backgroundEnqueueCount > 0
            ? debug.backgroundWriteCount >= backgroundCommands.length
            : true
        },
        {
          timeout: 10_000,
          message: 'Queued background terminal output did not drain through the scheduler'
        }
      )
      .toBe(true)

    const debug = await getSchedulerDebug(orcaPage)
    expect(debug.foregroundWriteCount).toBeGreaterThan(0)
    if (debug.drainWrites.length > 0) {
      expect(Math.max(...debug.drainWrites)).toBeLessThanOrEqual(2)
    }

    const firstBackground = backgroundCommands[0]
    const firstBackgroundTabId = tabIds[1]
    await tabLocator(orcaPage, firstBackgroundTabId).click()
    await expect
      .poll(() => getDomActiveTabId(orcaPage), {
        timeout: 5_000,
        message: 'Background terminal tab did not become active for content verification'
      })
      .toBe(firstBackgroundTabId)
    await expect
      .poll(async () => (await getTerminalContent(orcaPage)).includes(firstBackground.marker), {
        timeout: 5_000,
        message: 'Background terminal output was not preserved after scheduler drain'
      })
      .toBe(true)
  })

  test('visible bulk output uses the high-priority drain instead of synchronous xterm writes', async ({
    orcaPage
  }, testInfo) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)
    await ensureTerminalVisible(orcaPage)
    await waitForActiveTerminalManager(orcaPage, 30_000)

    const activeTabId = await createTerminalTab(orcaPage)
    if (!activeTabId) {
      throw new Error('Expected a fresh terminal tab')
    }
    const ptyId = await waitForTabPtyId(orcaPage, activeTabId)
    // Why: the freshly spawned shell must be input-ready before the flood, or its
    // leading bytes are dropped and the node command silently fails — the 700KB
    // throughput payload then never runs. Reset the scheduler counters AFTER the
    // sentinel round-trip so they reflect only the flood under test.
    await waitForShellInputReady(orcaPage, ptyId)
    await resetSchedulerDebug(orcaPage)

    const runId = Date.now()
    const marker = `VISIBLE_THROUGHPUT_${runId}`
    const floodCommand = nodeScriptCommand(
      `const marker='VISIBLE' + '_THROUGHPUT_' + '${runId}'; process.stdout.write('VISIBLE_FILL_${runId}\\n' + 'x'.repeat(700000) + '\\n' + marker + '\\n')`
    )

    await sendPtyCommands(orcaPage, [{ ptyId, command: floodCommand }])

    await expect
      .poll(async () => (await getTerminalContent(orcaPage, 12_000)).includes(marker), {
        timeout: 30_000,
        message: 'Active terminal did not render the visible throughput marker'
      })
      .toBe(true)

    const debug = await getSchedulerDebug(orcaPage)
    await testInfo.attach('terminal-visible-throughput-proof', {
      body: JSON.stringify(debug, null, 2),
      contentType: 'application/json'
    })
    testInfo.annotations.push({
      type: 'terminal-visible-throughput',
      description: `foreground=${debug.foregroundWriteCount} deferredForegroundEnqueue=${debug.deferredForegroundEnqueueCount} deferredForegroundWrite=${debug.deferredForegroundWriteCount} drains=${debug.drainWrites.join(',')}`
    })
    expect(debug.deferredForegroundEnqueueCount).toBeGreaterThan(0)
    expect(debug.deferredForegroundWriteCount).toBeGreaterThan(0)
  })

  test('hidden overflow restores from main-owned terminal state when the tab becomes visible', async ({
    orcaPage
  }) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)
    await ensureTerminalVisible(orcaPage)
    await waitForActiveTerminalManager(orcaPage, 30_000)

    const foregroundTabId = await getActiveTabId(orcaPage)
    if (!foregroundTabId) {
      throw new Error('Expected an initial terminal tab')
    }
    const hiddenTabId = await createTerminalTab(orcaPage)
    await waitForActiveTerminalManager(orcaPage, 30_000)
    const hiddenPtyId = await waitForTabPtyId(orcaPage, hiddenTabId)

    await tabLocator(orcaPage, foregroundTabId).click()
    await expect
      .poll(() => getDomActiveTabId(orcaPage), {
        timeout: 5_000,
        message: 'Foreground terminal tab did not become active before hidden flood'
      })
      .toBe(foregroundTabId)

    const marker = `HIDDEN_RECOVERY_${Date.now()}`
    const floodCommand = nodeScriptCommand(
      `for (let i = 0; i < 55000; i++) console.log('RECOVER_FILL_' + i + '_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'); console.log('${marker}')`
    )

    await sendPtyCommands(orcaPage, [{ ptyId: hiddenPtyId, command: floodCommand }])

    await expect
      .poll(async () => mainSnapshotContains(orcaPage, hiddenPtyId, marker), {
        timeout: 30_000,
        message: 'Main-owned terminal snapshot did not capture the hidden flood marker'
      })
      .toBe(true)

    await tabLocator(orcaPage, hiddenTabId).click()
    await expect
      .poll(() => getDomActiveTabId(orcaPage), {
        timeout: 5_000,
        message: 'Hidden terminal tab did not become visible for recovery verification'
      })
      .toBe(hiddenTabId)

    await expect
      .poll(async () => (await getTerminalContent(orcaPage)).includes(marker), {
        timeout: 10_000,
        message: 'Hidden terminal did not restore the marker from main-owned state'
      })
      .toBe(true)

    expect(await getTerminalContent(orcaPage)).not.toContain('Orca skipped hidden terminal output')
  })
})
