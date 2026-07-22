import { GitFork, Play, Plus, ShieldAlert } from 'lucide-react'
import {
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger
} from '@/components/ui/dropdown-menu'
import type { TerminalQuickCommand } from '../../../../shared/types'
import { isTerminalAgentQuickCommand } from '@/lib/git-wasm/terminal-quick-commands'
import { AgentIcon } from '@/lib/agent-catalog'
import { translate } from '@/i18n/i18n'

type TerminalQuickCommandsSubmenuProps = {
  repoQuickCommands: TerminalQuickCommand[]
  globalQuickCommands: TerminalQuickCommand[]
  // Why: project (orca.yaml) entries stay a separate list so provenance and the
  // trust gate can never be lost by merging them into user-owned commands.
  projectQuickCommands: TerminalQuickCommand[]
  projectQuickCommandsTrusted: boolean
  onReviewProjectQuickCommands: () => void
  quickCommandRepoLabel: string | null
  onQuickCommand: (command: TerminalQuickCommand) => void
  onAddQuickCommand: () => void
  onOpenChange: (open: boolean) => void
}

export function TerminalQuickCommandsSubmenu({
  repoQuickCommands,
  globalQuickCommands,
  projectQuickCommands,
  projectQuickCommandsTrusted,
  onReviewProjectQuickCommands,
  quickCommandRepoLabel,
  onQuickCommand,
  onAddQuickCommand,
  onOpenChange
}: TerminalQuickCommandsSubmenuProps): React.JSX.Element {
  const hasRepoGroupCommands = repoQuickCommands.length > 0 || projectQuickCommands.length > 0
  const hasQuickCommands = hasRepoGroupCommands || globalQuickCommands.length > 0

  const renderQuickCommandItem = (
    command: TerminalQuickCommand,
    provenance?: { project: boolean }
  ): React.JSX.Element => (
    <DropdownMenuItem
      key={command.id}
      // Why: repo-controlled orca.yaml entries stay inert until the shared-command trust hash is approved.
      disabled={provenance?.project === true && !projectQuickCommandsTrusted}
      onSelect={() => onQuickCommand(command)}
    >
      {isTerminalAgentQuickCommand(command) ? (
        <span className="flex size-3.5 shrink-0 items-center justify-center text-muted-foreground">
          <AgentIcon agent={command.agent} size={14} />
        </span>
      ) : (
        <Play
          className="size-3.5 shrink-0 text-muted-foreground"
          fill="currentColor"
          strokeWidth={0}
        />
      )}
      <span className="min-w-0 flex-1 truncate">{command.label}</span>
      {provenance?.project === true ? (
        <GitFork
          aria-label={translate(
            'auto.components.terminal.pane.TerminalContextMenu.projectQuickCommandProvenance',
            'Defined in orca.yaml'
          )}
          className="size-3 shrink-0 text-muted-foreground"
        />
      ) : null}
      {!isTerminalAgentQuickCommand(command) && !command.appendEnter ? (
        <DropdownMenuShortcut className="shrink-0">
          {translate('auto.components.terminal.pane.TerminalContextMenu.c2f0b72b8d', 'Insert')}
        </DropdownMenuShortcut>
      ) : null}
    </DropdownMenuItem>
  )

  return (
    <DropdownMenuSub>
      <DropdownMenuSubTrigger>
        <Play fill="currentColor" strokeWidth={0} />
        {translate('auto.components.terminal.pane.TerminalContextMenu.ec85df5914', 'Quick Commands')}
      </DropdownMenuSubTrigger>
      <DropdownMenuSubContent className="w-60">
        {hasQuickCommands ? (
          <>
            {quickCommandRepoLabel && hasRepoGroupCommands ? (
              <>
                <DropdownMenuLabel className="truncate">{quickCommandRepoLabel}</DropdownMenuLabel>
                {repoQuickCommands.map((command) => renderQuickCommandItem(command))}
                {projectQuickCommands.map((command) =>
                  renderQuickCommandItem(command, { project: true })
                )}
                {projectQuickCommands.length > 0 && !projectQuickCommandsTrusted ? (
                  <DropdownMenuItem
                    onSelect={() => {
                      // Why: the trust review opens a dialog; force-close the
                      // menu first (same overlay-guard pattern as Set Title).
                      onOpenChange(false)
                      onReviewProjectQuickCommands()
                    }}
                  >
                    <ShieldAlert />
                    {translate(
                      'auto.components.terminal.pane.TerminalContextMenu.reviewOrcaYamlTrust',
                      'Review orca.yaml trust…'
                    )}
                  </DropdownMenuItem>
                ) : null}
              </>
            ) : null}
            {globalQuickCommands.length > 0 ? (
              <>
                {hasRepoGroupCommands ? <DropdownMenuSeparator /> : null}
                {hasRepoGroupCommands ? (
                  <DropdownMenuLabel>
                    {translate('auto.components.terminal.pane.TerminalContextMenu.3ce594a4a0', 'Global')}
                  </DropdownMenuLabel>
                ) : null}
                {globalQuickCommands.map((command) => renderQuickCommandItem(command))}
              </>
            ) : null}
          </>
        ) : (
          <DropdownMenuItem disabled className="text-muted-foreground">
            {translate(
              'auto.components.terminal.pane.TerminalContextMenu.9528a65ef8',
              'No quick commands'
            )}
          </DropdownMenuItem>
        )}
        <DropdownMenuSeparator />
        <DropdownMenuItem
          onSelect={() => {
            // Why: the dropdown sits above dialogs; force-close before
            // opening the add modal even during the open-gesture guard.
            onOpenChange(false)
            onAddQuickCommand()
          }}
        >
          <Plus />
          {translate(
            'auto.components.terminal.pane.TerminalContextMenu.0a82b0608c',
            'Add Quick Command…'
          )}
        </DropdownMenuItem>
      </DropdownMenuSubContent>
    </DropdownMenuSub>
  )
}
