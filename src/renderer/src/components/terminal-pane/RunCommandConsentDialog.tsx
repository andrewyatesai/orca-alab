import { useSyncExternalStore } from 'react'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { useAppStore } from '@/store'
import { translate } from '@/i18n/i18n'
import {
  getPendingRunCommandConsent,
  settleRunCommandConsent,
  subscribeRunCommandConsent
} from '@/lib/deep-link-consent-gate'
import { runDeepLinkCommandInNewTab } from '@/lib/deep-link-run-command'
import {
  formatDeepLinkOriginLabel,
  showDeepLinkUnknownWorkspaceToast
} from '@/lib/deep-link-ui-notices'

/** Modal consent for `orca://run` deep links (#4384 §6.1). Always shown — no
 *  bypass setting and deliberately NO "always allow": a remembered grant would
 *  convert a one-time click into a persistent RCE primitive. Driven by the
 *  consent-gate module so OS-routed and terminal-clicked links share one path. */
export default function RunCommandConsentDialog(): React.JSX.Element {
  const request = useSyncExternalStore(subscribeRunCommandConsent, getPendingRunCommandConsent)
  const worktree = useAppStore((state) =>
    request ? state.getKnownWorktreeById(request.link.worktreeId) : undefined
  )
  const originWorktreeName = useAppStore((state) =>
    request?.origin.source === 'terminal'
      ? state.getKnownWorktreeById(request.origin.worktreeId)?.displayName
      : undefined
  )

  const handleConfirm = (): void => {
    if (request && runDeepLinkCommandInNewTab(request.link) === null) {
      // The worktree can vanish between consent opening and the confirm click.
      showDeepLinkUnknownWorkspaceToast()
    }
    settleRunCommandConsent()
  }

  const hostId = worktree?.hostId ?? 'local'

  return (
    <Dialog
      open={request !== null}
      onOpenChange={(isOpen) => {
        if (!isOpen) {
          settleRunCommandConsent()
        }
      }}
    >
      <DialogContent
        className="max-w-md"
        showCloseButton={false}
        // Why: Enter must never confirm running a command (§6.1) — Cancel holds focus, and this kills Enter even after a Tab.
        onKeyDown={(event) => {
          if (event.key === 'Enter') {
            event.preventDefault()
          }
        }}
      >
        <DialogHeader>
          <DialogTitle className="text-sm">
            {translate(
              'auto.components.terminal.pane.RunCommandConsentDialog.title',
              'Run this command?'
            )}
          </DialogTitle>
          <DialogDescription className="text-xs">
            {formatDeepLinkOriginLabel(
              request?.origin ?? { source: 'os' },
              originWorktreeName ?? null
            )}
          </DialogDescription>
        </DialogHeader>
        {/* Why: the executed text is shown in full — what is shown is exactly what runs (§6.1). */}
        <pre className="max-h-48 overflow-auto scrollbar-sleek rounded-md bg-muted p-2 font-mono text-xs whitespace-pre-wrap break-all select-text">
          {request?.link.command ?? ''}
        </pre>
        <div className="space-y-1 text-xs text-muted-foreground">
          <div>
            {translate(
              'auto.components.terminal.pane.RunCommandConsentDialog.targetWorkspace',
              'Workspace: {{name}}',
              { name: worktree?.displayName ?? request?.link.worktreeId ?? '' }
            )}
          </div>
          {worktree?.path ? (
            <div className="break-all">
              {translate(
                'auto.components.terminal.pane.RunCommandConsentDialog.targetPath',
                'Path: {{path}}',
                { path: worktree.path }
              )}
            </div>
          ) : null}
          <div>
            {/* Why: SSH/remote worktrees execute elsewhere — "Run command" must never be ambiguous about WHERE (§8). */}
            {hostId === 'local'
              ? translate(
                  'auto.components.terminal.pane.RunCommandConsentDialog.hostLocal',
                  'Runs on: this computer'
                )
              : translate(
                  'auto.components.terminal.pane.RunCommandConsentDialog.hostRemote',
                  'Runs on: {{host}}',
                  { host: hostId }
                )}
          </div>
        </div>
        <DialogFooter className="gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            autoFocus
            onClick={() => settleRunCommandConsent()}
          >
            {translate('auto.components.terminal.pane.RunCommandConsentDialog.cancel', 'Cancel')}
          </Button>
          <Button type="button" variant="destructive" size="sm" onClick={handleConfirm}>
            {translate(
              'auto.components.terminal.pane.RunCommandConsentDialog.confirm',
              'Run command'
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
