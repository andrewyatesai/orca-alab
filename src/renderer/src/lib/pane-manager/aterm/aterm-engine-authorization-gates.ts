import type { AtermTerminal } from './aterm_wasm'
import type { AtermPaneController } from './aterm-pane-controller-types'

/** The engine's fail-closed authorization gates the host toggles from user
 *  settings: OSC 52 clipboard writes, OSC 9/99/777 notifications, and the
 *  host-minted extra OSC-8 hyperlink scheme (deep-links #4384). Extracted from
 *  the wiring to keep it focused. */
export type AtermEngineAuthorizationGateMembers = Pick<
  AtermPaneController,
  'setClipboardWriteAuthorized' | 'setNotificationsAuthorized' | 'setHyperlinkSchemeAuthorized'
>

export function buildAtermEngineAuthorizationGates(
  term: AtermTerminal
): AtermEngineAuthorizationGateMembers {
  return {
    // Toggle the engine's fail-closed OSC 52 write gate so it queues OSC 52 set
    // events for the facade to drain; the host still enforces the user setting.
    setClipboardWriteAuthorized: (allowed: boolean) =>
      allowed ? term.authorize_clipboard_write() : term.revoke_clipboard_write(),
    // Engine-side fail-closed OSC 9/99/777 gate, synced from the user's notification
    // settings by the lifecycle layer (mirrors the OSC 52 clipboard gate above).
    setNotificationsAuthorized: (allowed: boolean) => term.authorize_notifications(allowed),
    // Host-minted extra OSC-8 scheme (deep-links #4384). Feature-detected so a
    // pre-capability wasm blob degrades to unlinkified orca:// text, not a crash.
    setHyperlinkSchemeAuthorized: (scheme: string) => {
      const authorize = (term as { authorize_hyperlink_scheme?: (scheme: string) => boolean })
        .authorize_hyperlink_scheme
      authorize?.call(term, scheme)
    }
  }
}
