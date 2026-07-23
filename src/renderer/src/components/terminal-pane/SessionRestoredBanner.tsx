import { translate } from '@/i18n/i18n'
import { SESSION_RESTORED_BANNER_ACTION_ATTRIBUTE } from './session-restored-banner-pane-state'

export const SESSION_RESTORED_BANNER_TEXT = '--- session restored ---'

type SessionRestoredBannerProps = {
  visible: boolean
  /** Last command the restored session ran (#7596); null hides the affordance. */
  lastCommand?: string | null
  /** Types the command into the pane WITHOUT executing (no trailing newline). */
  onTypeItAgain?: (command: string) => void
}

export function SessionRestoredBanner({
  visible,
  lastCommand = null,
  onTypeItAgain
}: SessionRestoredBannerProps): React.JSX.Element | null {
  if (!visible) {
    return null
  }

  if (!lastCommand) {
    return <div className="session-restored-banner">{SESSION_RESTORED_BANNER_TEXT}</div>
  }

  return (
    <div className="session-restored-banner session-restored-banner--with-command">
      <span className="session-restored-banner-text">
        {'--- '}
        {translate(
          'auto.components.terminal-pane.SessionRestoredBanner.lastRan',
          'session restored · last ran: {{command}}',
          { command: lastCommand }
        )}
        {' ---'}
      </span>
      <button
        type="button"
        className="session-restored-banner-action"
        // Why: the capture-phase dismiss listener exempts this attribute so the
        // click can reach us before the banner unmounts.
        {...{ [SESSION_RESTORED_BANNER_ACTION_ATTRIBUTE]: '' }}
        onClick={() => onTypeItAgain?.(lastCommand)}
      >
        {translate(
          'auto.components.terminal-pane.SessionRestoredBanner.typeItAgain',
          'Type it again'
        )}
      </button>
    </div>
  )
}
