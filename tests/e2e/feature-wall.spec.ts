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
    await expect(dialog.getByText('11 guided screens · about 4 minutes')).toBeVisible()
    await expect(dialog.getByText('Reopen any time from Help > Explore Orca.')).toBeVisible()

    const rail = dialog.getByRole('navigation', { name: 'Workflows' })
    await expect(rail.getByRole('tab')).toHaveCount(6)
    await expect(rail.getByRole('tab', { name: /Start/i })).toHaveAttribute('aria-selected', 'true')
    await expect(
      dialog.getByRole('heading', { name: 'Resume in the active workspace terminal' })
    ).toBeVisible()
    await expect(dialog.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '1')
    await expect(dialog.getByRole('progressbar')).toHaveAttribute('aria-valuemax', '11')
    await expect(dialog.getByRole('progressbar', { name: 'Tour progress' })).toHaveAttribute(
      'aria-valuetext',
      '1 of 11'
    )
    await expect(dialog.getByRole('button', { name: 'Learn more' })).toBeVisible()
    await expect(dialog.getByText('aterm · Rust')).toBeVisible()
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

    await rail.getByRole('tab', { name: /Plan/i }).click()
    await expect(dialog.getByText(/Connect the providers you use/)).toBeVisible()
    await rail.getByRole('button', { name: /Workspaces/i }).click()
    await expect(dialog.getByText(/parallel work does not collide/)).toBeVisible()

    await rail.getByRole('tab', { name: /Build/i }).click()
    await expect(dialog.getByText(/working, waiting, blocked, or done/)).toBeVisible()
    await rail.getByRole('button', { name: /Workbench/i }).click()
    await expect(dialog.getByText(/embedded browser/)).toBeVisible()

    await rail.getByRole('tab', { name: /Ship/i }).click()
    await expect(dialog.getByText(/connected Git provider/)).toBeVisible()

    await rail.getByRole('tab', { name: /Scale/i }).click()
    await expect(dialog.getByText(/dependent tasks/)).toBeVisible()
    await rail.getByRole('button', { name: /Automations/i }).click()
    await expect(dialog.getByText(/Remote targets must be reachable/)).toBeVisible()

    await rail.getByRole('tab', { name: /Anywhere/i }).click()
    await expect(dialog.getByText(/After one-time pairing/)).toBeVisible()
    await rail.getByRole('button', { name: /Computer Use/i }).click()
    await expect(dialog.getByText('Beta')).toHaveCount(1)
    await expect(dialog.getByText(/native helpers per platform/)).toBeVisible()
    await expect(
      dialog.getByText(/On macOS, grant Accessibility and Screen Recording/)
    ).toBeVisible()
    await expect(dialog.getByText(/on every platform, check capabilities/)).toBeVisible()
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
    expect(visibleVisualHeight).toBeGreaterThanOrEqual(16)

    await expect(workflowRow).toHaveAttribute('aria-orientation', 'horizontal')
    await rail.getByRole('tab', { name: /Scale/i }).focus()
    await orcaPage.keyboard.press('ArrowRight')
    await expect(rail.getByRole('tab', { name: /Anywhere/i })).toHaveAttribute(
      'aria-selected',
      'true'
    )
    await expect(stepRow).toBeVisible()

    await rail.getByRole('tab', { name: /Build/i }).click()
    await expectVisualToFitWidth(dialog.locator('[data-feature-wall-agents-visual="statuses"]'))
    await rail.getByRole('tab', { name: /Scale/i }).click()
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
    await expect(dialog.getByRole('button', { name: 'Return to Orca' })).toBeVisible()

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
    await rail.getByRole('button', { name: /Workspaces/i }).click()
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
      rail.getByRole('button', { name: /Workspaces/i }).locator('[aria-label="Viewed"]')
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
