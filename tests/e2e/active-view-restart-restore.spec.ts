/**
 * A full app restart opens the primary terminal while preserving Tasks substate.
 *
 * Restart behavior lives in E2E because it needs the real persisted UI and
 * workspace-session round-trip across two Electron launches. A real worktree
 * proves startup renders the terminal rather than the no-workspace Landing page.
 */

import { existsSync, readFileSync } from 'node:fs'
import type { ElectronApplication, Page } from '@stablyai/playwright-test'
import { test, expect } from './helpers/orca-app'
import { getStoreState, waitForSessionReady } from './helpers/store'
import { attachRepoAndOpenTerminal, createRestartSession } from './helpers/orca-restart'
import { TEST_REPO_PATH_FILE } from './global-setup'

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
    // And the terminal grid is not the active surface.
    await expect(first.page.locator('.xterm')).not.toBeVisible({ timeout: 10_000 })

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
