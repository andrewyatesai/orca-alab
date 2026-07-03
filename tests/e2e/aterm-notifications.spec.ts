import { test, expect } from './helpers/orca-app'
import type { ElectronApplication } from '@stablyai/playwright-test'
import {
  sendToTerminal,
  waitForActivePanePtyId,
  waitForActiveTerminalManager,
  waitForTerminalOutput
} from './helpers/terminal'
import { ensureTerminalVisible, waitForActiveWorktree, waitForSessionReady } from './helpers/store'

// OSC 9 desktop notifications: the aterm engine's fail-closed queue is authorized
// from the user's notification settings, drained after each processed chunk, and
// dispatched through the SAME notifications:dispatch IPC seam as the long-command
// feature. The e2e harness can't observe real OS notifications, so assert at that
// dispatch boundary (the droid-notification pattern), including that revoking the
// setting closes the engine gate again.

type NotificationDispatch = {
  source?: string
  paneKey?: string
  appNotificationTitle?: string | null
  appNotificationBody?: string | null
  appNotificationUrgency?: string
}

async function installMainProcessNotificationDispatchSpy(app: ElectronApplication): Promise<void> {
  await app.evaluate(({ ipcMain }) => {
    const g = globalThis as unknown as {
      __notificationDispatchLog?: NotificationDispatch[]
      __notificationDispatchSpyInstalled?: boolean
    }
    if (g.__notificationDispatchSpyInstalled) {
      return
    }
    g.__notificationDispatchLog = []
    g.__notificationDispatchSpyInstalled = true
    ipcMain.removeHandler('notifications:dispatch')
    ipcMain.handle('notifications:dispatch', (_event: unknown, args: NotificationDispatch) => {
      g.__notificationDispatchLog!.push(args)
      return { delivered: true }
    })
  })
}

async function getNotificationDispatches(
  app: ElectronApplication
): Promise<NotificationDispatch[]> {
  return app.evaluate(() => {
    const g = globalThis as unknown as { __notificationDispatchLog?: NotificationDispatch[] }
    return g.__notificationDispatchLog ?? []
  })
}

test.describe('aterm OSC 9 app notifications', () => {
  test('an authorized OSC 9 post reaches the dispatch seam; revoking the setting closes the gate', async ({
    orcaPage,
    electronApp
  }) => {
    await waitForSessionReady(orcaPage)
    await waitForActiveWorktree(orcaPage)
    await ensureTerminalVisible(orcaPage)
    await waitForActiveTerminalManager(orcaPage, 30_000)
    // Why: contextBridge freezes window.api, so notification invokes must be
    // observed in Electron's main process rather than monkey-patched renderer-side.
    await installMainProcessNotificationDispatchSpy(electronApp)

    const ptyId = await waitForActivePanePtyId(orcaPage)
    const readyMarker = `__OSC9_READY_${Date.now()}__`
    await sendToTerminal(orcaPage, ptyId, `printf '${readyMarker}\\n'\r`)
    await waitForTerminalOutput(orcaPage, readyMarker)

    // The default-ON setting authorizes the engine gate → the body-only OSC 9
    // payload is drained and dispatched with the pane-title fallback left to main.
    const body = `OSC9 ping ${Date.now()}`
    await sendToTerminal(orcaPage, ptyId, `printf '\\033]9;${body}\\007'\r`)
    await expect
      .poll(
        async () =>
          (await getNotificationDispatches(electronApp)).filter(
            (dispatch) =>
              dispatch.source === 'terminal-app-notification' &&
              dispatch.appNotificationBody === body
          ),
        {
          timeout: 30_000,
          message: 'authorized OSC 9 did not reach the notifications:dispatch seam'
        }
      )
      .toEqual([
        expect.objectContaining({
          source: 'terminal-app-notification',
          appNotificationTitle: null,
          appNotificationBody: body,
          appNotificationUrgency: 'normal',
          paneKey: expect.stringContaining(':')
        })
      ])

    // Flip the toggle OFF: the lifecycle re-applies the engine authorization, so
    // subsequent OSC 9 posts are dropped inside the engine (fail-closed again).
    await orcaPage.evaluate(async () => {
      const state = window.__store!.getState()
      await state.updateSettings({
        notifications: { ...state.settings!.notifications, terminalAppNotifications: false }
      })
    })
    const revokedBody = `OSC9 revoked ${Date.now()}`
    const revokedMarker = `__OSC9_REVOKED_${Date.now()}__`
    await sendToTerminal(
      orcaPage,
      ptyId,
      `printf '\\033]9;${revokedBody}\\007' && printf '${revokedMarker}\\n'\r`
    )
    // The trailing marker proves the OSC 9 bytes were parsed before we assert.
    await waitForTerminalOutput(orcaPage, revokedMarker)
    await orcaPage.waitForTimeout(500)
    expect(
      (await getNotificationDispatches(electronApp)).filter(
        (dispatch) => dispatch.appNotificationBody === revokedBody
      )
    ).toEqual([])
  })
})
