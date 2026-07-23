import { createPortal } from 'react-dom'
import { SessionRestoredBanner } from './SessionRestoredBanner'
import type {
  SessionRestoredBannerPane,
  SessionRestoredBannerState
} from './session-restored-banner-pane-state'

type SessionRestoredBannerPortalsProps = {
  panes: readonly SessionRestoredBannerPane[]
  states: ReadonlyMap<number, SessionRestoredBannerState>
  /** #7596: pane dismissal after the affordance typed the command. */
  onTypeItAgain: (pane: SessionRestoredBannerPane, command: string) => void
}

export function SessionRestoredBannerPortals({
  panes,
  states,
  onTypeItAgain
}: SessionRestoredBannerPortalsProps): React.JSX.Element {
  return (
    <>
      {panes.map((pane) => {
        const state = states.get(pane.id)
        if (!state) {
          return null
        }
        return createPortal(
          // Why: resumed TUIs repaint xterm immediately, so the wake marker
          // must live in that pane's chrome instead of the PTY byte stream.
          <SessionRestoredBanner
            visible
            lastCommand={state.lastCommand}
            onTypeItAgain={(command) => onTypeItAgain(pane, command)}
          />,
          pane.container,
          `session-restored-banner-${pane.id}`
        )
      })}
    </>
  )
}
