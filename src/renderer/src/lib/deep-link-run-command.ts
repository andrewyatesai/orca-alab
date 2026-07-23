import { useAppStore } from '@/store'
import { reconcileTabOrder } from '@/components/tab-bar/reconcile-order'
import { activateAndRevealWorktree } from '@/lib/worktree-activation'

export type DeepLinkRunCommandArgs = {
  worktreeId: string
  command: string
  title?: string
}

/**
 * Execute a consent-confirmed `orca://run` link: spawn a FRESH terminal tab in
 * the target worktree and queue the command as its startup command (mirrors
 * `runQuickCommandInNewTab`). The command is never written into an existing
 * PTY's stdin — no write-to-foreground-agent primitive (#4384 §6.1).
 */
export function runDeepLinkCommandInNewTab(args: DeepLinkRunCommandArgs): { tabId: string } | null {
  if (!args.command.trim()) {
    return null
  }
  // Why: unknown worktree (deleted since consent opened) must refuse, not spawn an orphan tab.
  if (activateAndRevealWorktree(args.worktreeId) === false) {
    return null
  }
  const store = useAppStore.getState()
  const tab = store.createTab(args.worktreeId)
  store.queueTabStartupCommand(tab.id, { command: args.command })
  const title = args.title?.trim()
  if (title) {
    store.setTabCustomTitle(tab.id, title)
  }
  // Why: match createNewTerminalTab — a worktree showing an editor would otherwise keep the new terminal invisible.
  store.setActiveTabType('terminal')

  // Why: persist tab-bar order with the new terminal appended, else reconcileTabOrder jumps it to index 0.
  const fresh = useAppStore.getState()
  const termIds = (fresh.tabsByWorktree[args.worktreeId] ?? []).map((t) => t.id)
  const editorIds = fresh.openFiles
    .filter((f) => f.worktreeId === args.worktreeId)
    .map((f) => f.id)
  const browserIds = (fresh.browserTabsByWorktree?.[args.worktreeId] ?? []).map((t) => t.id)
  const base = reconcileTabOrder(
    fresh.tabBarOrderByWorktree[args.worktreeId],
    termIds,
    editorIds,
    browserIds
  )
  const order = base.filter((id) => id !== tab.id)
  order.push(tab.id)
  fresh.setTabBarOrder(args.worktreeId, order)

  return { tabId: tab.id }
}
