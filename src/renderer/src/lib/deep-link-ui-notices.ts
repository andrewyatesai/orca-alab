import { toast } from 'sonner'
import { translate } from '@/i18n/i18n'
import type { OrcaDeepLinkOrigin } from '../../../shared/orca-deep-link'

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

export function showDeepLinkUnknownWorkspaceToast(): void {
  toast.error(
    translate(
      'auto.lib.deep.link.ui.notices.unknownWorkspace',
      'The workspace in this Orca link was not found'
    )
  )
}

/** Shown when a second `orca://run` link arrives while a consent dialog is
 *  already open — the new request is dropped, never swapped in (#4384 §6.1). */
export function showDeepLinkRunIgnoredToast(): void {
  toast.warning(
    translate(
      'auto.lib.deep.link.ui.notices.runIgnored',
      'Another Orca run link was ignored while a confirmation is open'
    )
  )
}

export function showDeepLinkPairRoutedToast(origin: OrcaDeepLinkOrigin): void {
  toast.info(
    translate(
      'auto.lib.deep.link.ui.notices.pairRouted',
      'Orca pairing is set up from Settings — pairing links never connect automatically'
    ),
    { description: formatDeepLinkOriginLabel(origin) }
  )
}

/** Provenance label for consent surfaces and toasts (#4384 §6.2). Origin is
 *  stamped by the transport, never parsed from the URL. */
export function formatDeepLinkOriginLabel(
  origin: OrcaDeepLinkOrigin,
  originWorktreeName?: string | null
): string {
  if (origin.source === 'terminal') {
    const name = originWorktreeName?.trim() || origin.worktreeId
    return translate(
      'auto.lib.deep.link.ui.notices.originTerminal',
      'Clicked in terminal output of "{{name}}" — terminal output is untrusted',
      { name }
    )
  }
  return translate(
    'auto.lib.deep.link.ui.notices.originOs',
    'Opened from outside Orca (a browser or another application)'
  )
}
