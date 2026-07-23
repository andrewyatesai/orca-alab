import { useAppStore } from '@/store'
import { activateAndRevealWorktree } from '@/lib/worktree-activation'
import { activateTabAndFocusPane } from '@/lib/activate-tab-and-focus-pane'
import {
  requestRunCommandConsent,
  runOrDeferDeepLinkNavigation
} from '@/lib/deep-link-consent-gate'
import {
  showDeepLinkPairRoutedToast,
  showDeepLinkRunIgnoredToast,
  showDeepLinkTerminalGoneToast,
  showDeepLinkUnknownWorkspaceToast,
  showDeepLinkUnrecognizedToast,
  showDeepLinkUnsupportedToast
} from '@/lib/deep-link-ui-notices'
import type {
  OrcaDeepLink,
  OrcaDeepLinkOrigin,
  OrcaDeepLinkUiEvent
} from '../../../shared/orca-deep-link'

/** Renderer-side dispatch for `worktree`/`pair`/`run` deep links (#4384 PR2).
 *  Reached from the main→renderer `ui:deepLink` channel (OS-routed links, origin
 *  stamped `os` by main) and from in-pane OSC-8 clicks (origin stamped
 *  `terminal` by the click path). No renderer input can forge an origin. */
export function dispatchDeepLinkInRenderer(link: OrcaDeepLink, origin: OrcaDeepLinkOrigin): void {
  switch (link.kind) {
    case 'worktree':
      // Why: navigation is held while a run-consent dialog is open (§6.3) — it must not re-target the UI mid-decision.
      runOrDeferDeepLinkNavigation(() => {
        if (activateAndRevealWorktree(link.worktreeId) === false) {
          showDeepLinkUnknownWorkspaceToast()
          return
        }
        if (link.tabId) {
          activateTabAndFocusPane(link.tabId, null)
        }
      })
      return
    case 'pair':
      // Why: desktop MINTS pairing offers (QR/URL) — it cannot consume a pair code, so the link
      // routes to the pairing pane and the code is deliberately dropped; never auto-pair (§6.1).
      runOrDeferDeepLinkNavigation(() => {
        const store = useAppStore.getState()
        store.openSettingsPage()
        store.openSettingsTarget({ pane: 'mobile', repoId: null })
        showDeepLinkPairRoutedToast(origin)
      })
      return
    case 'run': {
      // Why: a whitespace-only command would consent-prompt for a blank tab; treat as malformed.
      if (!link.command.trim()) {
        showDeepLinkUnrecognizedToast()
        return
      }
      if (!useAppStore.getState().getKnownWorktreeById(link.worktreeId)) {
        showDeepLinkUnknownWorkspaceToast()
        return
      }
      if (!requestRunCommandConsent({ link, origin })) {
        showDeepLinkRunIgnoredToast()
      }
      return
    }
    case 'focus':
      // Focus is dispatched in main (OS path) or in the pane click path; reaching here is a wiring bug.
      showDeepLinkUnsupportedToast()
  }
}

/** Renderer sink for the main→renderer `ui:deepLink` channel. */
export function handleDeepLinkUiEvent(event: OrcaDeepLinkUiEvent): void {
  if (event.type === 'link') {
    dispatchDeepLinkInRenderer(event.link, event.origin)
    return
  }
  switch (event.notice) {
    case 'unrecognized':
      showDeepLinkUnrecognizedToast()
      return
    case 'unsupported':
      showDeepLinkUnsupportedToast()
      return
    case 'terminal-gone':
      showDeepLinkTerminalGoneToast()
  }
}
