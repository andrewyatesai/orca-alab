import type { Repo, TerminalQuickCommand } from '../../../../shared/types'
import {
  getTerminalQuickCommandBody,
  isTerminalAgentQuickCommand
} from '@/lib/git-wasm/terminal-quick-commands'
import { useProjectQuickCommandsByRepo } from '@/hooks/use-project-quick-commands'
import { AgentIcon, getAgentLabel } from '@/lib/agent-catalog'
import { cn } from '@/lib/utils'
import { translate } from '@/i18n/i18n'
import { Badge } from '../ui/badge'
import { Label } from '../ui/label'
import { RepoBadgeMark } from '../repo/RepoBadgeLabel'
import { getQuickCommandRepoLabel } from './QuickCommandsScopeFilter'

function ProjectQuickCommandRow({
  command,
  repo
}: {
  command: TerminalQuickCommand
  repo: Pick<Repo, 'displayName' | 'path' | 'badgeColor'>
}): React.JSX.Element {
  return (
    <div className="flex items-center gap-3 rounded-md border border-border/60 bg-background px-3 py-2 shadow-xs">
      <div className="min-w-0 flex-1">
        <div className="flex min-w-0 items-center gap-2">
          <div className="truncate text-sm font-medium">{command.label}</div>
          <Badge variant="outline" className="max-w-44 gap-1.5">
            <RepoBadgeMark color={repo.badgeColor} />
            <span className="truncate">{getQuickCommandRepoLabel(repo)}</span>
          </Badge>
          <Badge variant="outline" className="shrink-0 font-mono">
            {translate('auto.components.settings.QuickCommandsPane.projectSourceBadge', 'orca.yaml')}
          </Badge>
        </div>
        <div className="flex min-w-0 items-center gap-1.5 text-xs text-foreground/80">
          {isTerminalAgentQuickCommand(command) ? (
            <span className="shrink-0 text-muted-foreground">
              <AgentIcon agent={command.agent} size={12} />
            </span>
          ) : null}
          <span className={cn('truncate', isTerminalAgentQuickCommand(command) ? '' : 'font-mono')}>
            {isTerminalAgentQuickCommand(command)
              ? `${getAgentLabel(command.agent)}: ${getTerminalQuickCommandBody(command)}`
              : getTerminalQuickCommandBody(command)}
          </span>
        </div>
      </div>
    </div>
  )
}

/** Read-only listing of orca.yaml project quick commands (#8481) — edits go through git. */
export function ProjectQuickCommandsSection({
  repos,
  effectiveSelection,
  showAll
}: {
  repos: Repo[]
  effectiveSelection: ReadonlySet<string>
  showAll: boolean
}): React.JSX.Element | null {
  const projectCommandsByRepo = useProjectQuickCommandsByRepo(repos.map((repo) => repo.id))
  const visibleRepos = repos.filter(
    (repo) =>
      (showAll || effectiveSelection.has(repo.id)) &&
      (projectCommandsByRepo.get(repo.id)?.length ?? 0) > 0
  )
  if (visibleRepos.length === 0) {
    return null
  }

  return (
    <div className="space-y-2">
      <div className="space-y-1 pt-2">
        <Label>
          {translate('auto.components.settings.QuickCommandsPane.projectCommandsTitle', 'Project Commands')}
        </Label>
        <p className="text-xs text-muted-foreground">
          {translate(
            'auto.components.settings.QuickCommandsPane.projectCommandsCaption',
            'Defined in orca.yaml and shared with everyone on the repository. Edit them in the repository, not here.'
          )}
        </p>
      </div>
      <div className="overflow-hidden rounded-lg border border-border/50 bg-muted/20">
        <div className="max-h-[40vh] space-y-2 overflow-y-auto p-2 scrollbar-sleek">
          {visibleRepos.flatMap((repo) =>
            (projectCommandsByRepo.get(repo.id) ?? []).map((command) => (
              <ProjectQuickCommandRow key={command.id} command={command} repo={repo} />
            ))
          )}
        </div>
      </div>
    </div>
  )
}
