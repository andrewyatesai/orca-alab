// The #9279 target-conditional context-menu items: rendered only when the
// menu-open capture found a selection, a link/path under the cursor, or a
// completed OSC-133 block. Split from TerminalContextMenu (line cap), following
// the TerminalQuickCommandsSubmenu precedent.

import { Copy, ExternalLink, FolderOpen, ScrollText, Search } from 'lucide-react'
import { DropdownMenuItem, DropdownMenuSeparator } from '@/components/ui/dropdown-menu'
import { translate } from '@/i18n/i18n'
import { isMacPlatform } from '../native-chat/native-chat-shortcut'

/** "Copy Last Command Output" (CM-A3); the caller hides it without a block. */
export function TerminalMenuCommandOutputItem({
  onCopyLastCommandOutput
}: {
  onCopyLastCommandOutput: () => void
}): React.JSX.Element {
  return (
    <DropdownMenuItem onSelect={onCopyLastCommandOutput}>
      <ScrollText />
      {translate(
        'auto.components.terminal.pane.TerminalContextMenu.copyLastCommandOutput',
        'Copy Last Command Output'
      )}
    </DropdownMenuItem>
  )
}

/** "Search for …" (CM-A1). The action searches the full captured first line;
 *  only the LABEL is ellipsized. */
export function TerminalMenuSearchSelectionItem({
  selectionSnippet,
  onSearchSelection
}: {
  selectionSnippet: string
  onSearchSelection: () => void
}): React.JSX.Element {
  const selectionLabel =
    selectionSnippet.length > 24 ? `${selectionSnippet.slice(0, 24)}…` : selectionSnippet
  return (
    <DropdownMenuItem onSelect={onSearchSelection}>
      <Search />
      <span className="truncate">
        {translate(
          'auto.components.terminal.pane.TerminalContextMenu.searchForSelection',
          'Search for “{{selection}}”',
          { selection: selectionLabel }
        )}
      </span>
    </DropdownMenuItem>
  )
}

/** Open / Copy / Reveal for the link or file-path target captured at the
 *  right-click point (CM-A2). Reveal is local-only — the caller gates it. */
export function TerminalMenuLinkTargetItems({
  linkTargetKind,
  onOpenLinkTarget,
  onCopyLinkTarget,
  canRevealLinkTarget,
  onRevealLinkTarget
}: {
  linkTargetKind: 'url' | 'osc8' | 'file' | 'provider'
  onOpenLinkTarget: () => void
  onCopyLinkTarget: () => void
  canRevealLinkTarget: boolean
  onRevealLinkTarget: () => void
}): React.JSX.Element {
  const isFile = linkTargetKind === 'file'
  const isWindowsPlatform =
    typeof navigator !== 'undefined' && navigator.userAgent.includes('Windows')
  const revealLabel = isMacPlatform()
    ? translate(
        'auto.components.terminal.pane.TerminalContextMenu.revealInFinder',
        'Reveal in Finder'
      )
    : isWindowsPlatform
      ? translate(
          'auto.components.terminal.pane.TerminalContextMenu.revealInFileExplorer',
          'Reveal in File Explorer'
        )
      : translate(
          'auto.components.terminal.pane.TerminalContextMenu.revealInFileManager',
          'Reveal in File Manager'
        )
  return (
    <>
      <DropdownMenuSeparator />
      <DropdownMenuItem onSelect={onOpenLinkTarget}>
        <ExternalLink />
        {isFile
          ? translate('auto.components.terminal.pane.TerminalContextMenu.openFileTarget', 'Open File')
          : translate('auto.components.terminal.pane.TerminalContextMenu.openLinkTarget', 'Open Link')}
      </DropdownMenuItem>
      <DropdownMenuItem onSelect={onCopyLinkTarget}>
        <Copy />
        {isFile
          ? translate('auto.components.terminal.pane.TerminalContextMenu.copyPathTarget', 'Copy Path')
          : translate('auto.components.terminal.pane.TerminalContextMenu.copyLinkTarget', 'Copy Link')}
      </DropdownMenuItem>
      {canRevealLinkTarget ? (
        <DropdownMenuItem onSelect={onRevealLinkTarget}>
          <FolderOpen />
          {revealLabel}
        </DropdownMenuItem>
      ) : null}
      <DropdownMenuSeparator />
    </>
  )
}
