/**
 * A full app restart opens the primary terminal while preserving Tasks substate.
 *
 * Restart behavior lives in E2E because it needs the real persisted UI and
 * workspace-session round-trip across two Electron launches. Coverage includes
 * both a real worktree and the zero-project Floating Workspace fallback.
 */

import { existsSync, readFileSync } from 'node:fs'
import type { ElectronApplication, Page } from '@stablyai/playwright-test'
import { test, expect } from './helpers/orca-app'
import { getStoreState, waitForSessionReady } from './helpers/store'
import { attachRepoAndOpenTerminal, createRestartSession } from './helpers/orca-restart'
import { TEST_REPO_PATH_FILE } from './global-setup'

const OPEN_FLOATING_WORKSPACE_SELECTOR = '[data-floating-terminal-panel][aria-hidden="false"]'

function seededRepoPathOrSkip(): string {
  const repoPath = existsSync(TEST_REPO_PATH_FILE)
    ? readFileSync(TEST_REPO_PATH_FILE, 'utf-8').trim()
    : ''
  test.skip(!repoPath || !existsSync(repoPath), 'Global setup did not produce a seeded test repo')
  return repoPath
}

async function readPersistedActiveView(page: Page): Promise<string | undefined> {
  return page.evaluate(() => window.api.ui.get().then((ui) => ui.activeView))
}

async function readPersistedGitHubMode(page: Page): Promise<string | undefined> {
  return page.evaluate(() => window.api.ui.get().then((ui) => ui.taskResumeState?.githubMode))
}

async function waitForTwoAnimationFrames(page: Page): Promise<void> {
  await page.evaluate(
    () =>
      new Promise<void>((resolve) => {
        requestAnimationFrame(() => requestAnimationFrame(() => resolve()))
      })
  )
}

test('opens the scratch terminal after first-run onboarding is skipped', async (// oxlint-disable-next-line no-empty-pattern -- Playwright's second fixture arg is testInfo; the first must be an object destructure to opt out of the default fixture set.
{}, testInfo) => {
  test.setTimeout(180_000)
  const session = createRestartSession(testInfo, { seedCompletedOnboarding: false })
  let app: ElectronApplication | null = null
  try {
    const launched = await session.launch()
    app = launched.app
    const onboarding = launched.page.getByRole('dialog', { name: 'Orca onboarding' })
    await expect(onboarding).toBeVisible({ timeout: 30_000 })

    await launched.page.keyboard.press('Escape')
    const confirmation = launched.page.getByRole('dialog', { name: 'Skip onboarding?' })
    await expect(confirmation).toBeVisible()
    await confirmation.getByRole('button', { name: 'Skip', exact: true }).click()
    await expect(onboarding).not.toBeVisible()
    await waitForSessionReady(launched.page)

    const panel = launched.page.locator(OPEN_FLOATING_WORKSPACE_SELECTOR)
    await expect(panel).toBeVisible({ timeout: 30_000 })
    await expect(panel.locator('.xterm').first()).toBeVisible({ timeout: 30_000 })
    await expect(panel.locator('[data-testid="sortable-tab"]')).toHaveCount(1)
  } finally {
    if (app) {
      try {
        await session.close(app)
      } catch {
        // best-effort cleanup
      }
    }
    await session.dispose()
  }
})

test('opens one maximized local scratch terminal across no-project restarts', async (// oxlint-disable-next-line no-empty-pattern -- Playwright's second fixture arg is testInfo; the first must be an object destructure to opt out of the default fixture set.
{}, testInfo) => {
  test.setTimeout(300_000)
  const session = createRestartSession(testInfo)
  let firstApp: ElectronApplication | null = null
  let secondApp: ElectronApplication | null = null
  try {
    const first = await session.launch()
    firstApp = first.app
    await waitForSessionReady(first.page)

    const firstPanel = first.page.locator(OPEN_FLOATING_WORKSPACE_SELECTOR)
    await expect(firstPanel).toBeVisible({ timeout: 30_000 })
    await expect(
      firstPanel.getByRole('button', { name: 'Restore floating workspace' })
    ).toBeVisible()
    await expect(firstPanel.locator('.xterm').first()).toBeVisible({ timeout: 30_000 })
    const firstTabs = firstPanel.locator('[data-testid="sortable-tab"]')
    await expect(firstTabs).toHaveCount(1)
    await expect(firstTabs.first()).toHaveAttribute('data-tab-title', 'Terminal 1')
    const firstTabId = await firstTabs.first().getAttribute('data-tab-id')
    expect(firstTabId).not.toBeNull()

    await firstPanel.getByRole('button', { name: 'Minimize floating workspace' }).click()
    const retainedPanel = first.page.locator('[data-floating-terminal-panel]')
    await expect(retainedPanel).toHaveAttribute('aria-hidden', 'true')
    await waitForTwoAnimationFrames(first.page)
    // The startup decision is one-shot; closing the panel must not immediately reopen it.
    await expect(retainedPanel).toHaveAttribute('aria-hidden', 'true')

    await session.close(firstApp)
    firstApp = null

    const second = await session.launch()
    secondApp = second.app
    await waitForSessionReady(second.page)

    const secondPanel = second.page.locator(OPEN_FLOATING_WORKSPACE_SELECTOR)
    await expect(secondPanel).toBeVisible({ timeout: 30_000 })
    await expect(
      secondPanel.getByRole('button', { name: 'Restore floating workspace' })
    ).toBeVisible()
    await expect(secondPanel.locator('.xterm').first()).toBeVisible({ timeout: 30_000 })
    const restoredTabs = secondPanel.locator('[data-testid="sortable-tab"]')
    await expect(restoredTabs).toHaveCount(1)
    await expect(restoredTabs.first()).toHaveAttribute('data-tab-id', firstTabId!)
  } finally {
    for (const app of [secondApp, firstApp]) {
      if (!app) {
        continue
      }
      try {
        await session.close(app)
      } catch {
        // best-effort cleanup
      }
    }
    await session.dispose()
  }
})

test('opens terminal after restart and preserves the Tasks project mode', async (// oxlint-disable-next-line no-empty-pattern -- Playwright's second fixture arg is testInfo; the first must be an object destructure to opt out of the default fixture set.
{}, testInfo) => {
  test.setTimeout(300_000)
  const repoPath = seededRepoPathOrSkip()
  const session = createRestartSession(testInfo)
  let firstApp: ElectronApplication | null = null
  let secondApp: ElectronApplication | null = null
  try {
    const first = await session.launch()
    firstApp = first.app
    await waitForSessionReady(first.page)
    // Attach a repo + open its terminal so there is an active worktree; without
    // one the app renders Landing instead of the view switch. This also settles
    // startup worktree activation before we navigate.
    await attachRepoAndOpenTerminal(first.page, repoPath)

    // Precondition: attaching lands on the terminal.
    expect(await getStoreState<string>(first.page, 'activeView')).toBe('terminal')

    // Seed the persisted Project submode before mounting Tasks. The DOM below
    // proves TaskPage consumed it rather than only checking Zustand state.
    await first.page.evaluate(() => {
      const store = window.__store
      if (!store) {
        throw new Error('window.__store is not available')
      }
      store.getState().setTaskResumeState({ githubMode: 'project' })
      store.getState().openTaskPage()
    })
    await expect
      .poll(async () => getStoreState<string>(first.page, 'activeView'), { timeout: 10_000 })
      .toBe('tasks')
    await expect(
      first.page.locator('[data-contextual-tour-target="tasks-source-filters"]')
    ).toBeVisible({ timeout: 10_000 })
    await expect(first.page.getByRole('button', { name: 'Choose a project' })).toBeVisible({
      timeout: 10_000
    })
    // The retained project xterm is hidden; the floating scratch terminal is independent.
    await expect(first.page.locator('.xterm').first()).not.toBeVisible({ timeout: 10_000 })

    // The debounced writer must flush the view to the main-process UI state
    // before we quit, so the relaunch reads it back from disk.
    await expect
      .poll(async () => readPersistedActiveView(first.page), { timeout: 10_000 })
      .toBe('tasks')
    await expect
      .poll(async () => readPersistedGitHubMode(first.page), { timeout: 10_000 })
      .toBe('project')

    await session.close(firstApp)
    firstApp = null

    // Relaunch against the same userDataDir — the real reload/restore path.
    const second = await session.launch()
    secondApp = second.app
    await waitForSessionReady(second.page)

    // Startup ignores the persisted secondary page and opens the primary workbench.
    await expect
      .poll(async () => getStoreState<string>(second.page, 'activeView'), { timeout: 10_000 })
      .toBe('terminal')
    await expect(second.page.locator('.xterm').first()).toBeVisible({ timeout: 10_000 })
    await expect(
      second.page.locator('[data-contextual-tour-target="tasks-source-filters"]')
    ).not.toBeVisible({ timeout: 10_000 })

    // Projects remain available and reopen in the persisted submode.
    await second.page.locator('[data-contextual-tour-target="sidebar-tasks"]').click()
    await expect(second.page.getByRole('button', { name: 'Choose a project' })).toBeVisible({
      timeout: 10_000
    })
  } finally {
    // Guard each step so a failing close still runs the remaining cleanup.
    for (const app of [secondApp, firstApp]) {
      if (!app) {
        continue
      }
      try {
        await session.close(app)
      } catch {
        // best-effort cleanup
      }
    }
    await session.dispose()
  }
})
