// Context-menu link-target + last-command-output behaviors (#9279 / CM-A2, A3):
// capture at menu open, and the open / copy / reveal / copy-output actions the
// menu items dispatch. Split from use-terminal-pane-context-menu so the new
// actions stay unit-testable without mounting the whole pane hook.

import { toast } from 'sonner'
import type { ManagedPane } from '@/lib/pane-manager/pane-manager'
import type { AtermContextLinkTarget } from '@/lib/pane-manager/aterm/aterm-link-input'
import { getConnectionId } from '@/lib/connection-context'
import { getRuntimeEnvironmentIdForWorktree } from '@/lib/worktree-runtime-owner'
import { useAppStore } from '@/store'
import { translate } from '@/i18n/i18n'
import { copyTerminalTextVerified } from './terminal-copy-outcome'

export type { AtermContextLinkTarget } from '@/lib/pane-manager/aterm/aterm-link-input'

/** What the menu-open event captures for the clicked pane. The selection is read
 *  synchronously (Critic: it can be cleared between right-click and menu paint);
 *  the link target and command-output presence resolve async via `apply`, which
 *  the hook gates on its open-sequence so a late resolve for a closed/reopened
 *  menu is dropped. */
export function captureTerminalMenuTargets(
  pane: ManagedPane | null,
  point: { clientX: number; clientY: number },
  apply: (partial: {
    linkTarget?: AtermContextLinkTarget | null
    hasCommandOutput?: boolean
  }) => void
): { selectionText: string } {
  const controller = pane?.atermController
  const selectionText = controller?.selectionText() ?? pane?.terminal.getSelection() ?? ''
  void controller?.contextLinkTargetAt?.(point.clientX, point.clientY).then((linkTarget) => {
    apply({ linkTarget })
  })
  void controller?.lastCommandOutputAsync?.().then((output) => {
    apply({ hasCommandOutput: output !== null })
  })
  return { selectionText }
}

/** Reveal is local-shell-only: SSH panes and remote runtimes have no local file
 *  for the OS file manager, so the item is hidden rather than left to fail. */
export function isTerminalMenuRevealAvailable(worktreeId: string): boolean {
  if (getConnectionId(worktreeId)) {
    return false
  }
  return getRuntimeEnvironmentIdForWorktree(useAppStore.getState(), worktreeId) === null
}

/** Open the captured target via the pane's routing (in-app preference,
 *  scheme-aware OSC-8, late-bound file opener, provider activate). */
export function openTerminalMenuLinkTarget(
  pane: ManagedPane | null,
  target: AtermContextLinkTarget | null
): void {
  if (!pane || !target) {
    return
  }
  pane.atermController?.openContextLinkTarget?.(target, { openWithSystemDefault: false })
  pane.terminal.focus()
}

/** Copy Link / Copy Path: file targets copy the RAW matched span (what the row
 *  showed), url/osc8 the resolved url, provider targets their link text. */
export async function copyTerminalMenuLinkTarget(
  pane: ManagedPane | null,
  target: AtermContextLinkTarget | null
): Promise<void> {
  if (!target) {
    return
  }
  const text =
    target.kind === 'file' ? target.rawPathText : target.kind === 'provider' ? target.text : target.url
  const ok = await copyTerminalTextVerified(text, 'context-menu')
  if (ok) {
    toast.success(
      target.kind === 'file'
        ? translate(
            'auto.components.terminal.pane.terminal.context.menu.link.target.pathCopied',
            'Path copied'
          )
        : translate(
            'auto.components.terminal.pane.terminal.context.menu.link.target.linkCopied',
            'Link copied'
          )
    )
  }
  pane?.terminal.focus()
}

/** Reveal a file target in the OS file manager. Resolves the raw span against
 *  the pane's live cwd/home; unresolvable / missing paths surface a toast. */
export async function revealTerminalMenuFileTarget(
  pane: ManagedPane | null,
  target: AtermContextLinkTarget | null
): Promise<void> {
  if (!pane || target?.kind !== 'file') {
    return
  }
  const absolutePath = pane.atermController?.contextFileLinkAbsolutePath?.(target.rawPathText) ?? null
  const result = absolutePath ? await window.api.shell.openInFileManager(absolutePath) : null
  if (!result?.ok) {
    toast.error(
      translate(
        'auto.components.terminal.pane.terminal.context.menu.link.target.revealFailed',
        'Unable to reveal the file — it may have moved or been deleted'
      )
    )
  }
  pane.terminal.focus()
}

/** Copy Last Command Output (CM-A3). Reads FRESH at click time — the block may
 *  have changed since menu open; the eviction marker surfaces honestly. */
export async function copyTerminalMenuLastCommandOutput(pane: ManagedPane | null): Promise<void> {
  if (!pane) {
    return
  }
  const output = (await pane.atermController?.lastCommandOutputAsync?.()) ?? null
  if (!output) {
    // Defensive: the item is hidden when no block completed.
    return
  }
  if (output.status === 'evicted') {
    toast.error(
      translate(
        'auto.components.terminal.pane.terminal.context.menu.link.target.outputEvicted',
        'Command output scrolled past the scrollback limit'
      )
    )
    pane.terminal.focus()
    return
  }
  const ok = await copyTerminalTextVerified(output.text, 'context-menu')
  if (ok) {
    toast.success(
      translate(
        'auto.components.terminal.pane.terminal.context.menu.link.target.outputCopied',
        'Command output copied'
      )
    )
  }
  pane.terminal.focus()
}
