import { test, expect } from './helpers/orca-app'
import { waitForSessionReady } from './helpers/store'
import type { ElectronApplication, Locator, Page } from '@stablyai/playwright-test'

async function openFeatureTourFromMenu(electronApp: ElectronApplication): Promise<void> {
  await electronApp.evaluate(({ BrowserWindow, Menu }) => {
    const featureTourItem = Menu.getApplicationMenu()
      ?.items.find((item) => item.label === 'Help')
      ?.submenu?.items.find((item) => item.label === 'Explore Orca')

    if (!featureTourItem) {
      throw new Error('Explore Orca menu item was not registered')
    }

    const window = BrowserWindow.getAllWindows()[0]
    featureTourItem.click(featureTourItem, window, {
      triggeredByAccelerator: false,
      shiftKey: false,
      metaKey: false,
      ctrlKey: false,
      altKey: false
    } as Electron.KeyboardEvent)
  })
}

async function clearWalkthroughProgress(orcaPage: Page): Promise<void> {
  await orcaPage.evaluate(() => {
    localStorage.removeItem('orca.featureWall.visitedWorkflows.v2')
    localStorage.removeItem('orca.featureWall.visitedSteps.v2')
  })
}

test.describe('ALab feature walkthrough', () => {
  test.beforeEach(async ({ orcaPage }) => {
    await waitForSessionReady(orcaPage)
    await clearWalkthroughProgress(orcaPage)
  })

  test('opens from Help with scope, branding, and terminal-first navigation', async ({
    electronApp,
    orcaPage
  }) => {
    await openFeatureTourFromMenu(electronApp)

    const dialog = orcaPage.getByRole('dialog', { name: 'Explore Orca: ALab Edition' })
    await expect(dialog).toBeVisible({ timeout: 10_000 })
    await expect(dialog.getByText('14 guided screens · about 7 minutes')).toBeVisible()
    await expect(dialog.getByText('Reopen any time from Help > Explore Orca.')).toBeVisible()

    const rail = dialog.getByRole('navigation', { name: 'Workflows' })
    await expect(rail.getByRole('tab')).toHaveCount(6)
    await expect(rail.getByRole('tab', { name: /Start/i })).toHaveAttribute('aria-selected', 'true')
    await expect(
      dialog.getByRole('heading', { name: 'Start in a terminal—project or scratch' })
    ).toBeVisible()
    await expect(dialog.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '1')
    await expect(dialog.getByRole('progressbar')).toHaveAttribute('aria-valuemax', '14')
    await expect(dialog.getByRole('progressbar', { name: 'Tour progress' })).toHaveAttribute(
      'aria-valuetext',
      '1 of 14'
    )
    await expect(dialog.getByRole('button', { name: 'Learn more' })).toBeVisible()
    await expect(dialog.getByText('aterm · Rust')).toBeVisible()
    await expect(dialog.getByText(/review then launch Quick Commands/)).toBeVisible()
    await expect(dialog.getByText(/Project commands from orca.yaml stay inert/)).toBeVisible()
    const continueButton = dialog.getByRole('button', { name: /^Continue/ })
    await expect(continueButton).toHaveAttribute('aria-keyshortcuts', /^(Meta|Control)\+Enter$/)
    await expect(continueButton).toContainText('Enter')

    await rail.getByRole('tab', { name: /Start/i }).focus()
    await orcaPage.keyboard.press('ArrowDown')
    await expect(rail.getByRole('tab', { name: /Plan/i })).toHaveAttribute('aria-selected', 'true')
    await expect(rail.getByRole('button', { name: /Tasks/i })).toHaveAttribute(
      'aria-current',
      'step'
    )
  })

  test('features every major workflow with accurate provider and platform copy', async ({
    electronApp,
    orcaPage
  }) => {
    await openFeatureTourFromMenu(electronApp)
    const dialog = orcaPage.getByRole('dialog', { name: 'Explore Orca: ALab Edition' })
    const rail = dialog.getByRole('navigation', { name: 'Workflows' })

    await rail.getByRole('button', { name: /Add a project/i }).click()
    const addProjectVisual = dialog.locator('[data-add-project-workflow="action-progress-result"]')
    await expect(addProjectVisual).toContainText('User action · Create workspace')
    await expect(addProjectVisual).toContainText('Approved shared setup runs')
    await expect(addProjectVisual).toContainText('Workspace ready')
    await expect(addProjectVisual).toContainText(/changes require re-review/)

    await rail.getByRole('tab', { name: /Plan/i }).click()
    await expect(dialog.getByText(/Connect the providers you use/)).toBeVisible()
    await expect(dialog.locator('[data-feature-wall-task-provider="github"]')).toContainText(
      'connected for tasks'
    )
    await expect(dialog.locator('[data-feature-wall-linked-issue-context]').first()).toContainText(
      'Linked issue'
    )
    await rail.getByRole('button', { name: /Race approaches/i }).click()
    await expect(dialog.getByText(/Workspace Board/)).toBeVisible()
    await expect(dialog.getByText(/status lanes/)).toBeVisible()
    await expect(dialog.getByText(/does not launch or merge the race/)).toBeVisible()
    await expect(dialog.getByRole('heading', { name: /keep the winner/ })).toBeVisible()

    await rail.getByRole('tab', { name: /Build/i }).click()
    await expect(dialog.getByText(/supported or custom terminal agents/)).toBeVisible()
    await expect(dialog.getByText('Agent Session History', { exact: true })).toBeVisible()
    await expect(
      dialog.getByText(/resume when the transcript has conversation content/)
    ).toBeVisible()
    await expect(
      dialog.getByText(/Neither makes worktrees a machine-security sandbox/)
    ).toBeVisible()
    await rail.getByRole('button', { name: /Workbench/i }).click()
    await expect(dialog.getByText(/Quick Open and the Jump Palette/)).toBeVisible()
    await expect(dialog.getByText(/default-on Floating Workspace/)).toBeVisible()
    await expect(dialog.getByText('Optional Voice Dictation', { exact: true })).toBeVisible()
    await rail.getByRole('button', { name: /Browser & Design Mode/i }).click()
    await expect(dialog.getByText(/DOM and computed styles/)).toBeVisible()
    await expect(
      dialog.getByText(/source hint and cropped screenshot when available/)
    ).toBeVisible()
    await expect(dialog.getByText(/Import cookies.*only when you choose/)).toBeVisible()

    await rail.getByRole('tab', { name: /Ship/i }).click()
    await expect(dialog.getByText(/return to the same workspace, resolve, and retry/)).toBeVisible()
    await expect(dialog.locator('[data-feature-wall-review-ship-visual]')).toContainText(
      'Stage focused hunk'
    )

    await rail.getByRole('tab', { name: /Scale/i }).click()
    await expect(dialog.getByText(/version-matched bundled/)).toBeVisible()
    await rail.getByRole('button', { name: /Orchestration/i }).click()
    await expect(dialog.getByText(/simple workspace race/)).toBeVisible()
    await expect(dialog.locator('[data-feature-wall-orchestration-story]')).toHaveAttribute(
      'data-feature-wall-story-loop',
      'once'
    )
    await rail.getByRole('button', { name: /Automations/i }).click()
    await expect(dialog.getByText(/optional precheck/)).toBeVisible()

    await rail.getByRole('tab', { name: /Anywhere/i }).click()
    await expect(dialog.getByText(/on-demand environment described by orca.yaml/)).toBeVisible()
    await rail.getByRole('button', { name: /App emulators/i }).click()
    await expect(dialog.getByText(/workspace-scoped iOS Simulator pane/)).toBeVisible()
    await expect(dialog.getByText(/macOS, Linux, or Windows/)).toBeVisible()
    await expect(dialog.getByText(/physical ADB device/)).toBeVisible()
    await expect(dialog.getByText(/Orca's workspace Emulator pane/)).toBeVisible()
    await expect(dialog.getByText(/iOS control is local to the Mac/)).toBeVisible()
    await expect(dialog.locator('[data-emulator-recovery-stage="verified-result"]')).toContainText(
      'Profile email · agent@example.com'
    )
    await rail.getByRole('button', { name: /Computer Use/i }).click()
    await expect(dialog.getByText('Beta')).toHaveCount(1)
    await expect(dialog.getByText(/native helpers per platform/)).toBeVisible()
    await expect(
      dialog.getByText(/On macOS, grant Accessibility and Screen Recording/)
    ).toBeVisible()
    await expect(dialog.getByText(/on every platform, check capabilities/)).toBeVisible()
    await expect(dialog.locator('[data-computer-use-stage="invoke"]')).toContainText('Reconnect')
    await expect(dialog.locator('[data-computer-use-stage="result"]')).toContainText(
      'Agent connected'
    )
  })

  test('keeps every screen outcome discoverable at the standard walkthrough size', async ({
    electronApp,
    orcaPage
  }) => {
    await orcaPage.setViewportSize({ width: 1272, height: 800 })
    await orcaPage.emulateMedia({ reducedMotion: 'reduce' })
    await openFeatureTourFromMenu(electronApp)

    const dialog = orcaPage.getByRole('dialog', { name: 'Explore Orca: ALab Edition' })
    const previewPanel = dialog.getByRole('tabpanel')
    const continueButton = dialog.getByRole('button', { name: /^Continue/ })
    const stepIds = [
      'terminal',
      'add-project',
      'tasks',
      'workspaces',
      'agents',
      'workbench',
      'browser-design',
      'review-ship',
      'cli-skills',
      'orchestration',
      'automations',
      'remote-mobile',
      'mobile-emulators',
      'computer-use'
    ]
    let clippedScreenCount = 0

    for (const [index, stepId] of stepIds.entries()) {
      const visual = dialog.locator(
        `[data-feature-wall-step-visual="${stepId}"] [data-feature-wall-visual-content]`
      )
      await expect(visual).toBeVisible()
      await expect.poll(() => previewPanel.evaluate((element) => element.scrollTop)).toBe(0)

      if (!(await isVisualBottomVisible(visual, previewPanel))) {
        clippedScreenCount += 1
        const scrollAffordance = dialog.locator('[data-feature-wall-scroll-affordance]')
        await expect(scrollAffordance).toBeVisible()
        const previousScrollTop = await previewPanel.evaluate((element) => element.scrollTop)
        await scrollAffordance.click()
        await expect
          .poll(() => previewPanel.evaluate((element) => element.scrollTop))
          .toBeGreaterThan(previousScrollTop)
        await expect(scrollAffordance).toHaveCount(0)
        await expect.poll(() => isVisualBottomVisible(visual, previewPanel)).toBe(true)
      }

      if (index < stepIds.length - 1) {
        await continueButton.click()
      }
    }

    // Why: this proves the test exercised the explicit overflow path instead
    // of passing only because a future layout happened to fit every screen.
    expect(clippedScreenCount).toBeGreaterThan(0)
  })

  test('keeps compact navigation and active content usable at the minimum window size', async ({
    electronApp,
    orcaPage
  }) => {
    await orcaPage.setViewportSize({ width: 600, height: 400 })
    await openFeatureTourFromMenu(electronApp)

    const dialog = orcaPage.getByRole('dialog', { name: 'Explore Orca: ALab Edition' })
    const rail = dialog.getByRole('navigation', { name: 'Workflows' })
    const workflowRow = rail.locator('[data-feature-wall-navigation-row="workflows"]')
    const stepRow = rail.locator('[data-feature-wall-navigation-row="steps"]')
    const previewPanel = dialog.getByRole('tabpanel')
    const learnMore = dialog.getByRole('button', { name: 'Learn more about Terminal first' })
    const footer = dialog.locator('footer')
    const visual = dialog.locator(
      '[data-feature-wall-step-visual="terminal"] [data-feature-wall-visual-content]'
    )
    const description = dialog.locator('[data-feature-wall-step-description]')
    const expandDescription = dialog.getByRole('button', { name: 'Show full description' })

    await expect(dialog).toBeVisible({ timeout: 10_000 })
    await expect(learnMore).toBeVisible()
    await expect(visual).toBeVisible()
    expect(await previewPanel.evaluate((element) => element.scrollTop)).toBe(0)
    const [panelBounds, learnMoreBounds, footerBounds, visualBounds] = await Promise.all([
      previewPanel.boundingBox(),
      learnMore.boundingBox(),
      footer.boundingBox(),
      visual.boundingBox()
    ])
    if (!panelBounds || !learnMoreBounds || !footerBounds || !visualBounds) {
      throw new Error('Compact walkthrough first-fold elements did not render')
    }
    expect(learnMoreBounds.y + learnMoreBounds.height).toBeLessThanOrEqual(footerBounds.y)
    const visibleVisualHeight =
      Math.min(visualBounds.y + visualBounds.height, panelBounds.y + panelBounds.height) -
      Math.max(visualBounds.y, panelBounds.y)
    // Why: the minimum window still promises a visual walkthrough, so a token
    // sliver below the prose does not count as a usable first-fold preview.
    expect(visibleVisualHeight).toBeGreaterThanOrEqual(72)

    await expect(expandDescription).toBeVisible()
    await expect(expandDescription).toHaveAttribute('aria-expanded', 'false')
    const descriptionId = await description.getAttribute('id')
    expect(descriptionId).not.toBeNull()
    await expect(expandDescription).toHaveAttribute('aria-controls', descriptionId ?? '')
    await expect(previewPanel).toHaveAttribute('aria-describedby', descriptionId ?? '')
    expect(
      await description.evaluate((element) => element.scrollHeight > element.clientHeight + 1)
    ).toBe(true)

    await expandDescription.click()
    const collapseDescription = dialog.getByRole('button', { name: 'Collapse description' })
    await expect(collapseDescription).toHaveAttribute('aria-expanded', 'true')
    await expect
      .poll(() =>
        description.evaluate((element) => element.scrollHeight <= element.clientHeight + 1)
      )
      .toBe(true)

    await expect(workflowRow).toHaveAttribute('aria-orientation', 'horizontal')
    await rail.getByRole('tab', { name: /Scale/i }).focus()
    await orcaPage.keyboard.press('ArrowRight')
    await expect(rail.getByRole('tab', { name: /Anywhere/i })).toHaveAttribute(
      'aria-selected',
      'true'
    )
    await expect(stepRow).toBeVisible()

    await rail.getByRole('tab', { name: /Build/i }).click()
    await expectVisualToFitWidth(
      dialog.locator('[data-feature-wall-step-visual="agents"] [data-feature-wall-visual-content]')
    )
    await rail.getByRole('tab', { name: /Scale/i }).click()
    await rail.getByRole('button', { name: /Orchestration/i }).click()
    await expectVisualToFitWidth(
      dialog.locator('[data-feature-wall-agents-visual="orchestration"]')
    )
    await rail.getByRole('tab', { name: /Anywhere/i }).click()

    const workflowRowBounds = await workflowRow.boundingBox()
    const stepRowBounds = await stepRow.boundingBox()
    if (!workflowRowBounds || !stepRowBounds) {
      throw new Error('Compact walkthrough navigation rows did not render')
    }
    expect(stepRowBounds.y + 1).toBeGreaterThanOrEqual(
      workflowRowBounds.y + workflowRowBounds.height
    )

    await stepRow.getByRole('button', { name: /Computer Use/i }).click()
    await expect(
      dialog.getByRole('heading', { name: 'Operate desktop apps with guardrails' })
    ).toBeVisible()
    await expect(dialog.locator('footer')).toBeVisible()
    const returnToOrca = dialog.getByRole('button', { name: 'Return to Orca' })
    const finalActions = returnToOrca.locator('..')
    await expect(returnToOrca).toBeVisible()
    await expect(finalActions.getByRole('button', { name: 'Finish setup' })).toBeVisible()
    await expect(finalActions.getByRole('button')).toHaveCount(2)

    await stepRow.getByRole('button', { name: /Remote & mobile/i }).click()
    await expect(
      dialog.getByRole('heading', { name: 'Keep work moving away from this machine' })
    ).toBeVisible()

    await stepRow.getByRole('button', { name: /Remote & mobile/i }).focus()
    await orcaPage.setViewportSize({ width: 1000, height: 700 })
    await expect
      .poll(() =>
        orcaPage.evaluate(() => {
          const active = document.activeElement as HTMLElement | null
          return {
            stepId: active?.dataset.featureWallStepId ?? null,
            visible: active?.offsetParent !== null
          }
        })
      )
      .toEqual({ stepId: 'remote-mobile', visible: true })

    await orcaPage.setViewportSize({ width: 600, height: 400 })
    await expect
      .poll(() =>
        orcaPage.evaluate(() => {
          const active = document.activeElement as HTMLElement | null
          return {
            stepId: active?.dataset.featureWallStepId ?? null,
            visible: active?.offsetParent !== null
          }
        })
      )
      .toEqual({ stepId: 'remote-mobile', visible: true })
  })

  test('completes through either final action', async ({ electronApp, orcaPage }) => {
    await openFeatureTourFromMenu(electronApp)
    let dialog = orcaPage.getByRole('dialog', { name: 'Explore Orca: ALab Edition' })
    let rail = dialog.getByRole('navigation', { name: 'Workflows' })
    await rail.getByRole('tab', { name: /Anywhere/i }).click()
    await rail.getByRole('button', { name: /Computer Use/i }).click()
    await dialog.getByRole('button', { name: 'Return to Orca' }).click()
    await expect(dialog).toHaveCount(0)

    await openFeatureTourFromMenu(electronApp)
    dialog = orcaPage.getByRole('dialog', { name: 'Explore Orca: ALab Edition' })
    rail = dialog.getByRole('navigation', { name: 'Workflows' })
    await rail.getByRole('tab', { name: /Anywhere/i }).click()
    await rail.getByRole('button', { name: /Computer Use/i }).click()
    await dialog.getByRole('button', { name: 'Finish setup' }).click()

    await expect(dialog).toHaveCount(0)
    await expect(orcaPage.getByRole('dialog', { name: 'Getting started' })).toBeVisible()
  })

  test('opens and closes without modal animation when reduced motion is requested', async ({
    electronApp,
    orcaPage
  }) => {
    await orcaPage.emulateMedia({ reducedMotion: 'reduce' })
    await orcaPage.evaluate(() => {
      document.body.dataset.featureWallModalMotionEvents = '0'
      document.addEventListener('animationstart', (event) => {
        const target = event.target
        if (
          target instanceof Element &&
          target.matches('[data-slot="dialog-content"], [data-slot="dialog-overlay"]')
        ) {
          const count = Number(document.body.dataset.featureWallModalMotionEvents ?? '0')
          document.body.dataset.featureWallModalMotionEvents = String(count + 1)
        }
      })
    })

    await openFeatureTourFromMenu(electronApp)
    const dialog = orcaPage.getByRole('dialog', { name: 'Explore Orca: ALab Edition' })
    const overlay = orcaPage.locator('[data-slot="dialog-overlay"]')
    await expect(dialog).toBeVisible({ timeout: 10_000 })
    await expect(dialog).toHaveCSS('animation-name', 'none')
    await expect(overlay).toHaveCSS('animation-name', 'none')

    const rail = dialog.getByRole('navigation', { name: 'Workflows' })
    await rail.getByRole('tab', { name: /Build/i }).click()
    await rail.getByRole('button', { name: /Browser & Design Mode/i }).click()
    const visualMotionIsSuppressed = await dialog
      .locator('[data-feature-wall-step-visual="browser-design"]')
      .evaluate((root) =>
        [root, ...root.querySelectorAll('*')].every((element) => {
          const style = getComputedStyle(element)
          // Why: CSS may retain duration metadata after transition-property disables all motion.
          const hasNoTransition =
            style.transitionProperty === 'none' ||
            style.transitionDuration
              .split(',')
              .every((duration) => Number.parseFloat(duration) === 0)

          return style.animationName === 'none' && hasNoTransition
        })
      )
    expect(visualMotionIsSuppressed).toBe(true)

    await orcaPage.keyboard.press('Escape')
    await expect(dialog).toHaveCount(0)
    expect(
      await orcaPage.locator('body').getAttribute('data-feature-wall-modal-motion-events')
    ).toBe('0')
  })

  test('moves rail focus with a shortcut across a chapter boundary', async ({
    electronApp,
    orcaPage
  }) => {
    await openFeatureTourFromMenu(electronApp)
    const dialog = orcaPage.getByRole('dialog', { name: 'Explore Orca: ALab Edition' })
    const rail = dialog.getByRole('navigation', { name: 'Workflows' })
    const addProject = rail.getByRole('button', { name: /Add a project/i })

    await addProject.click()
    await addProject.focus()
    await orcaPage.keyboard.press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter')

    await expect(rail.getByRole('tab', { name: /Plan/i })).toHaveAttribute('aria-selected', 'true')
    await expect(rail.getByRole('button', { name: /Tasks/i })).toBeFocused()
  })

  test('Continue and Back retain footer focus across chapter boundaries', async ({
    electronApp,
    orcaPage
  }) => {
    await openFeatureTourFromMenu(electronApp)
    const dialog = orcaPage.getByRole('dialog', { name: 'Explore Orca: ALab Edition' })
    const rail = dialog.getByRole('navigation', { name: 'Workflows' })
    const continueButton = dialog.getByRole('button', { name: /^Continue/ })

    await continueButton.focus()
    await orcaPage.keyboard.press('Enter')
    await expect(rail.getByRole('button', { name: /Add a project/i })).toHaveAttribute(
      'aria-current',
      'step'
    )
    await expect(continueButton).toBeFocused()

    await orcaPage.keyboard.press('Enter')
    await expect(rail.getByRole('tab', { name: /Plan/i })).toHaveAttribute('aria-selected', 'true')
    await expect(continueButton).toBeFocused()

    const backButton = dialog.getByRole('button', { name: 'Back' })
    await backButton.focus()
    await orcaPage.keyboard.press('Enter')
    await expect(rail.getByRole('tab', { name: /Start/i })).toHaveAttribute('aria-selected', 'true')
    await expect(backButton).toBeFocused()
  })

  test('persists viewed screens and chapter completion across Help replays', async ({
    electronApp,
    orcaPage
  }) => {
    await openFeatureTourFromMenu(electronApp)
    let dialog = orcaPage.getByRole('dialog', { name: 'Explore Orca: ALab Edition' })
    let rail = dialog.getByRole('navigation', { name: 'Workflows' })

    await rail.getByRole('tab', { name: /Plan/i }).click()
    await rail.getByRole('button', { name: /Race approaches/i }).click()
    await expect(
      rail.locator('[data-feature-wall-workflow-id="plan"] [aria-label="Viewed"]')
    ).toHaveCount(1)

    await orcaPage.keyboard.press('Escape')
    await expect(dialog).toHaveCount(0)
    await openFeatureTourFromMenu(electronApp)
    dialog = orcaPage.getByRole('dialog', { name: 'Explore Orca: ALab Edition' })
    rail = dialog.getByRole('navigation', { name: 'Workflows' })
    await expect(
      rail.locator('[data-feature-wall-workflow-id="plan"] [aria-label="Viewed"]')
    ).toHaveCount(1)

    await rail.getByRole('tab', { name: /Plan/i }).click()
    await expect(
      rail.getByRole('button', { name: /Tasks/i }).locator('[aria-label="Viewed"]')
    ).toHaveCount(1)
    await expect(
      rail.getByRole('button', { name: /Race approaches/i }).locator('[aria-label="Viewed"]')
    ).toHaveCount(1)
  })
})

async function expectVisualToFitWidth(visual: Locator): Promise<void> {
  await expect(visual).toBeAttached()
  const fits = await visual.evaluate((element) => {
    const parent = element.parentElement
    return parent !== null && element.getBoundingClientRect().width <= parent.clientWidth + 1
  })
  expect(fits).toBe(true)
}

async function isVisualBottomVisible(visual: Locator, viewport: Locator): Promise<boolean> {
  const [visualBounds, viewportBounds] = await Promise.all([
    visual.boundingBox(),
    viewport.boundingBox()
  ])
  if (!visualBounds || !viewportBounds) {
    return false
  }
  return visualBounds.y + visualBounds.height <= viewportBounds.y + viewportBounds.height + 1
}
