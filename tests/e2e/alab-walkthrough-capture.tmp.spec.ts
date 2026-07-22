import { mkdirSync } from 'node:fs'
import { resolve } from 'node:path'
import type { ElectronApplication } from '@stablyai/playwright-test'
import { test, expect } from './helpers/orca-app'
import { waitForSessionReady } from './helpers/store'

const SCREENS = [
  ['start', 'terminal', '01-terminal'],
  ['start', 'add-project', '02-add-project'],
  ['plan', 'tasks', '03-tasks'],
  ['plan', 'workspaces', '04-workspaces'],
  ['build', 'agents', '05-agents-attention'],
  ['build', 'workbench', '06-workbench'],
  ['build', 'browser-design', '07-browser-design'],
  ['ship', 'review-ship', '08-review-ship'],
  ['scale', 'cli-skills', '09-cli-skills'],
  ['scale', 'orchestration', '10-orchestration'],
  ['scale', 'automations', '11-automations'],
  ['anywhere', 'remote-mobile', '12-remote-mobile'],
  ['anywhere', 'mobile-emulators', '13-app-emulators'],
  ['anywhere', 'computer-use', '14-computer-use']
] as const

async function openFeatureTourFromMenu(electronApp: ElectronApplication): Promise<void> {
  await electronApp.evaluate(({ BrowserWindow, Menu }) => {
    const item = Menu.getApplicationMenu()
      ?.items.find((candidate) => candidate.label === 'Help')
      ?.submenu?.items.find((candidate) => candidate.label === 'Explore Orca')
    if (!item) {
      throw new Error('Explore Orca menu item was not registered')
    }
    item.click(item, BrowserWindow.getAllWindows()[0], {
      triggeredByAccelerator: false,
      shiftKey: false,
      metaKey: false,
      ctrlKey: false,
      altKey: false
    } as Electron.KeyboardEvent)
  })
}

test('captures every ALab walkthrough screen', async ({ electronApp, orcaPage }) => {
  test.setTimeout(180_000)
  await waitForSessionReady(orcaPage)
  await orcaPage.setViewportSize({ width: 1272, height: 800 })
  await orcaPage.emulateMedia({ reducedMotion: 'reduce' })
  await openFeatureTourFromMenu(electronApp)

  const outputDir = resolve('test-results/alab-walkthrough-final')
  mkdirSync(outputDir, { recursive: true })
  const dialog = orcaPage.getByRole('dialog', { name: 'Explore Orca: ALab Edition' })
  await expect(dialog).toBeVisible()

  for (const [workflowId, stepId, filename] of SCREENS) {
    await dialog.locator(`[data-feature-wall-workflow-id="${workflowId}"]`).click()
    await dialog.locator(`[data-feature-wall-step-id="${stepId}"]:visible`).click()
    await expect(dialog.locator(`[data-feature-wall-step-visual="${stepId}"]`)).toBeVisible()
    await dialog.screenshot({
      path: resolve(outputDir, `${filename}-overview.png`),
      animations: 'disabled'
    })
    const panel = dialog.getByRole('tabpanel')
    await panel.evaluate((element) => {
      element.scrollTop = element.scrollHeight
      element.dispatchEvent(new Event('scroll'))
    })
    await dialog.screenshot({
      path: resolve(outputDir, `${filename}-outcome.png`),
      animations: 'disabled'
    })
  }
})
