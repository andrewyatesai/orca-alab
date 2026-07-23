import { toast } from 'sonner'
import { translate } from '@/i18n/i18n'
import type { OrcaDeepLinkUiEvent } from '../../../shared/orca-deep-link'

export function showDeepLinkUnrecognizedToast(): void {
  toast.error(translate('auto.lib.deep.link.ui.notices.unrecognized', 'Unrecognized Orca link'))
}

export function showDeepLinkUnsupportedToast(): void {
  toast.info(
    translate(
      'auto.lib.deep.link.ui.notices.unsupported',
      'This Orca link type is not supported yet'
    )
  )
}

export function showDeepLinkTerminalGoneToast(): void {
  toast.error(
    translate('auto.lib.deep.link.ui.notices.terminalGone', 'Terminal is no longer running')
  )
}

/** Renderer sink for the main→renderer `ui:deepLink` channel. PR1 (#4384):
 *  `focus` is dispatched in main, so any forwarded `link` event is a
 *  not-yet-supported kind (worktree/pair/run land with PR2's consent surface). */
export function handleDeepLinkUiEvent(event: OrcaDeepLinkUiEvent): void {
  if (event.type === 'link') {
    showDeepLinkUnsupportedToast()
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
