import type { DaemonRuntimeStatus } from '../../../../preload/api-types'
import { translate } from '@/i18n/i18n'

export type DaemonStatusToastCopy = {
  title: string
  description: string
  actionLabel: string
}

/**
 * Localized copy for the sticky failure toast
 * (docs/reference/daemon-staleness-ux.md §Phase 2). Returns null for states that never
 * show a toast ('starting', 'running').
 */
export function getDaemonStatusToastCopy(status: DaemonRuntimeStatus): DaemonStatusToastCopy | null {
  if (status.state === 'failed') {
    return {
      title: translate(
        'auto.components.daemon.status.copy.failedTitle',
        'Terminal persistence unavailable'
      ),
      description: translate(
        'auto.components.daemon.status.copy.failedDescription',
        'The terminal daemon didn’t start. New terminals run in-process and won’t survive quitting Orca.'
      ),
      actionLabel: translate('auto.components.daemon.status.copy.retryAction', 'Retry')
    }
  }
  if (status.state !== 'degraded-fallback') {
    return null
  }
  if (status.cause === 'startup-timeout') {
    return {
      title: translate(
        'auto.components.daemon.status.copy.degradedTitle',
        'Terminal persistence degraded'
      ),
      description: translate(
        'auto.components.daemon.status.copy.timeoutDescription',
        'The terminal daemon didn’t finish starting in time, so new terminals won’t survive quitting Orca. Retrying closes open terminal panes.'
      ),
      actionLabel: translate('auto.components.daemon.status.copy.retryAction', 'Retry')
    }
  }
  return {
    title: translate(
      'auto.components.daemon.status.copy.degradedTitle',
      'Terminal persistence degraded'
    ),
    description: translate(
      'auto.components.daemon.status.copy.degradedDescription',
      'Existing terminals keep working, but new terminals won’t survive quitting Orca. Restarting the daemon closes open terminal panes.'
    ),
    actionLabel: translate('auto.components.daemon.status.copy.restartAction', 'Restart daemon')
  }
}

export function getDaemonStatusRestoredMessage(): string {
  return translate(
    'auto.components.daemon.status.copy.restoredMessage',
    'Terminal persistence restored.'
  )
}

export function getDaemonStatusRetryFailedTitle(): string {
  return translate('auto.components.daemon.status.copy.retryFailedTitle', 'Daemon restart failed.')
}

export type DaemonStatusIndicatorCopy = {
  label: string
  tooltip: string
  ariaLabel: string
}

/**
 * Localized copy for the low-key status-bar indicator. Returns null while the
 * daemon is running or still starting (nothing to indicate).
 */
export function getDaemonStatusIndicatorCopy(
  status: DaemonRuntimeStatus
): DaemonStatusIndicatorCopy | null {
  if (status.state !== 'failed' && status.state !== 'degraded-fallback') {
    return null
  }
  return {
    label: translate(
      'auto.components.daemon.status.copy.indicatorLabel',
      'Terminal persistence off'
    ),
    tooltip:
      status.state === 'failed'
        ? translate(
            'auto.components.daemon.status.copy.indicatorFailedTooltip',
            'The terminal daemon isn’t running, so new terminals won’t survive quitting Orca. Open Manage Sessions to restart it.'
          )
        : translate(
            'auto.components.daemon.status.copy.indicatorDegradedTooltip',
            'New terminals are running without daemon persistence. Open Manage Sessions to restart the daemon.'
          ),
    ariaLabel: translate(
      'auto.components.daemon.status.copy.indicatorAriaLabel',
      'Terminal daemon status: persistence off'
    )
  }
}
