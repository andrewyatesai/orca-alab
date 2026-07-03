import { useEffect, useRef, useState } from 'react'
import type { GlobalSettings } from '../../../../shared/types'
import {
  LONG_COMMAND_THRESHOLD_SECONDS_MAX,
  LONG_COMMAND_THRESHOLD_SECONDS_MIN
} from '../../../../shared/constants'
import { Button } from '../ui/button'
import { Input } from '../ui/input'
import { Label } from '../ui/label'
import { Separator } from '../ui/separator'
import { BellRing, Bot, Megaphone, Siren, Timer } from 'lucide-react'
import { useAppStore } from '@/store'
import { NotificationSettingToggle } from './NotificationSettingToggle'
import { NotificationSoundSection } from './NotificationSoundSection'
import {
  createNotificationVolumeDraftState,
  resolveNotificationVolumeDraftState,
  sendNotificationSettingsTestNotification
} from './notification-settings-copy'
import { translate } from '@/i18n/i18n'
export { getNotificationsPaneSearchEntries } from './notifications-search'
export {
  createNotificationVolumeDraftState,
  resolveNotificationVolumeDraftState,
  sendNotificationSettingsTestNotification
} from './notification-settings-copy'

type NotificationsPaneProps = {
  settings: GlobalSettings
  updateSettings: (updates: Partial<GlobalSettings>) => void | Promise<void>
}

export function NotificationsPane({
  settings,
  updateSettings
}: NotificationsPaneProps): React.JSX.Element {
  const notificationSettings = settings.notifications
  const notificationSettingsRef = useRef(notificationSettings)

  const updateNotificationSettings = async (
    updates: Partial<GlobalSettings['notifications']>
  ): Promise<void> => {
    const nextNotifications = {
      ...notificationSettingsRef.current,
      ...updates
    }
    notificationSettingsRef.current = nextNotifications
    await updateSettings({
      notifications: {
        ...nextNotifications
      }
    })
  }

  useEffect(() => {
    notificationSettingsRef.current = notificationSettings
  }, [notificationSettings])

  const [volumeDraftState, setVolumeDraftState] = useState(() =>
    createNotificationVolumeDraftState(notificationSettings.customSoundVolume)
  )
  const resolvedVolumeDraftState = resolveNotificationVolumeDraftState(
    volumeDraftState,
    notificationSettings.customSoundVolume
  )
  if (resolvedVolumeDraftState !== volumeDraftState) {
    setVolumeDraftState(resolvedVolumeDraftState)
  }
  const volumeDraft = resolvedVolumeDraftState.draft
  const setVolumeDraft = (value: number): void => {
    setVolumeDraftState((current) => ({
      ...resolveNotificationVolumeDraftState(current, notificationSettings.customSoundVolume),
      draft: value
    }))
  }

  const [thresholdDraft, setThresholdDraft] = useState(() =>
    String(notificationSettings.longCommandThresholdSeconds)
  )
  const lastCommittedThreshold = useRef(notificationSettings.longCommandThresholdSeconds)
  if (lastCommittedThreshold.current !== notificationSettings.longCommandThresholdSeconds) {
    // Why: an external settings update (another window, reset) must refresh the
    // draft; local typing alone must not, or every keystroke would be clamped.
    lastCommittedThreshold.current = notificationSettings.longCommandThresholdSeconds
    setThresholdDraft(String(notificationSettings.longCommandThresholdSeconds))
  }
  const commitThresholdDraft = (): void => {
    const parsed = Number.parseInt(thresholdDraft, 10)
    const clamped = Number.isNaN(parsed)
      ? notificationSettings.longCommandThresholdSeconds
      : Math.min(
          LONG_COMMAND_THRESHOLD_SECONDS_MAX,
          Math.max(LONG_COMMAND_THRESHOLD_SECONDS_MIN, parsed)
        )
    setThresholdDraft(String(clamped))
    if (clamped !== notificationSettings.longCommandThresholdSeconds) {
      lastCommittedThreshold.current = clamped
      void updateNotificationSettings({ longCommandThresholdSeconds: clamped })
    }
  }

  const handleVolumeCommit = (value: number): void => {
    if (notificationSettingsRef.current.customSoundVolume !== value) {
      void updateNotificationSettings({ customSoundVolume: value })
    }
  }

  const handleSendTestNotification = async (): Promise<void> => {
    useAppStore.getState().recordFeatureInteraction('notifications')
    await sendNotificationSettingsTestNotification(notificationSettings, volumeDraft)
  }

  return (
    <div className="space-y-1">
      <NotificationSettingToggle
        label={translate(
          'auto.components.settings.NotificationsPane.841c8c549f',
          'Enable Notifications'
        )}
        description={translate(
          'auto.components.settings.NotificationsPane.deff6d30da',
          'Native system notifications for background events.'
        )}
        checked={notificationSettings.enabled}
        onToggle={() => {
          if (!notificationSettings.enabled) {
            useAppStore.getState().recordFeatureInteraction('notifications')
          }
          void updateNotificationSettings({ enabled: !notificationSettings.enabled })
        }}
      />

      <Separator />

      <NotificationSettingToggle
        icon={<Bot className="size-4" />}
        label={translate(
          'auto.components.settings.NotificationsPane.ca76d06fd2',
          'Agent Task Complete'
        )}
        description={translate(
          'auto.components.settings.NotificationsPane.55f901a59b',
          'A coding agent finishes and becomes idle.'
        )}
        checked={notificationSettings.agentTaskComplete}
        disabled={!notificationSettings.enabled}
        onToggle={() =>
          void updateNotificationSettings({
            agentTaskComplete: !notificationSettings.agentTaskComplete
          })
        }
      />

      <NotificationSettingToggle
        icon={<Siren className="size-4" />}
        label={translate('auto.components.settings.NotificationsPane.591fe605b9', 'Terminal Bell')}
        description={translate(
          'auto.components.settings.NotificationsPane.b6fc369244',
          'A background terminal emits a bell character.'
        )}
        checked={notificationSettings.terminalBell}
        disabled={!notificationSettings.enabled}
        onToggle={() =>
          void updateNotificationSettings({
            terminalBell: !notificationSettings.terminalBell
          })
        }
      />

      <NotificationSettingToggle
        icon={<Megaphone className="size-4" />}
        label={translate(
          'auto.components.settings.NotificationsPane.922a54c037',
          'Terminal App Notifications'
        )}
        description={translate(
          'auto.components.settings.NotificationsPane.f627c38a77',
          'A terminal program posts a desktop notification (OSC 9).'
        )}
        checked={notificationSettings.terminalAppNotifications}
        disabled={!notificationSettings.enabled}
        onToggle={() =>
          void updateNotificationSettings({
            terminalAppNotifications: !notificationSettings.terminalAppNotifications
          })
        }
      />

      <NotificationSettingToggle
        icon={<Timer className="size-4" />}
        label={translate(
          'auto.components.settings.NotificationsPane.fb499fb735',
          'Long Command Finished'
        )}
        description={translate(
          'auto.components.settings.NotificationsPane.1ced11d6b5',
          'A command in a background terminal finishes after the time threshold.'
        )}
        checked={notificationSettings.longCommandComplete}
        disabled={!notificationSettings.enabled}
        onToggle={() =>
          void updateNotificationSettings({
            longCommandComplete: !notificationSettings.longCommandComplete
          })
        }
      />

      <div className="flex items-center justify-between gap-4 py-2">
        <div className="space-y-0.5">
          <Label>
            {translate(
              'auto.components.settings.NotificationsPane.08b271938b',
              'Long Command Threshold'
            )}
          </Label>
          <p className="text-xs text-muted-foreground">
            {translate(
              'auto.components.settings.NotificationsPane.162aeb3e20',
              'Minimum command run time before Orca notifies, in seconds.'
            )}
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Input
            type="number"
            min={LONG_COMMAND_THRESHOLD_SECONDS_MIN}
            max={LONG_COMMAND_THRESHOLD_SECONDS_MAX}
            step={1}
            disabled={!notificationSettings.enabled || !notificationSettings.longCommandComplete}
            value={thresholdDraft}
            onChange={(e) => setThresholdDraft(e.target.value)}
            onBlur={commitThresholdDraft}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                commitThresholdDraft()
              }
            }}
            aria-label={translate(
              'auto.components.settings.NotificationsPane.08b271938b',
              'Long Command Threshold'
            )}
            className="number-input-clean w-20 tabular-nums"
          />
          <span className="text-xs text-muted-foreground">
            {translate('auto.components.settings.NotificationsPane.e1f2188fcd', 'seconds')}
          </span>
        </div>
      </div>

      <Separator />

      <NotificationSoundSection
        notificationSettings={notificationSettings}
        notificationsEnabled={notificationSettings.enabled}
        volumeDraft={volumeDraft}
        onVolumeDraftChange={setVolumeDraft}
        onVolumeCommit={handleVolumeCommit}
        onUpdateNotificationSettings={updateNotificationSettings}
      />

      <Separator />

      <NotificationSettingToggle
        label={translate(
          'auto.components.settings.NotificationsPane.00cd406dbb',
          'Suppress While Focused'
        )}
        description={translate(
          'auto.components.settings.NotificationsPane.2772d2f257',
          'Skip notifications when the triggering worktree is already visible.'
        )}
        checked={notificationSettings.suppressWhenFocused}
        disabled={!notificationSettings.enabled}
        onToggle={() =>
          void updateNotificationSettings({
            suppressWhenFocused: !notificationSettings.suppressWhenFocused
          })
        }
      />

      <div className="flex flex-wrap items-center gap-2 pt-3">
        <Button
          variant="outline"
          size="sm"
          disabled={!notificationSettings.enabled}
          onClick={() => void handleSendTestNotification()}
          className="gap-2"
        >
          <BellRing className="size-3.5" />
          {translate(
            'auto.components.settings.NotificationsPane.906b4afebf',
            'Send Test Notification'
          )}
        </Button>
      </div>
    </div>
  )
}
