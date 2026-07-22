import type { JSX } from 'react'
import { Archive, ArrowRight, ExternalLink, FileClock, History, Search } from 'lucide-react'
import { translate } from '@/i18n/i18n'

export function AiVaultSessionHistoryStrip(): JSX.Element {
  return (
    <section
      className="border-t border-border bg-background/40 px-3.5 py-2"
      data-feature-wall-ai-vault="session-history"
    >
      <div className="flex min-w-0 items-center gap-2">
        <Archive className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
        <span className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
          {translate('auto.fw.agentAttention.vault.title', 'Agent Session History')}
        </span>
        <span className="ml-auto flex min-w-0 items-center gap-1 rounded-md border border-border bg-card px-2 py-1 text-[11px] text-muted-foreground">
          <span className="font-mono">1</span>
          <Search className="size-2.5 shrink-0" aria-hidden />
          <span className="truncate">
            {translate('auto.fw.agentAttention.vault.search', 'Workspace scope · reconnect')}
          </span>
        </span>
      </div>
      <div className="mt-1.5 flex min-w-0 flex-wrap items-center gap-1.5 text-[11px]">
        <ArrowRight className="size-3 shrink-0 text-muted-foreground" aria-hidden />
        <span className="inline-flex min-w-0 flex-1 items-center gap-1 truncate rounded-md border border-border bg-card px-1.5 py-1 font-medium">
          <span className="font-mono text-muted-foreground">2</span>
          {translate('auto.fw.agentAttention.vault.result', 'Codex · reconnect flow')}
        </span>
        <ArrowRight className="size-3 shrink-0 text-muted-foreground" aria-hidden />
        <span className="font-mono text-muted-foreground">3</span>
        <VaultAction
          icon={ExternalLink}
          label={translate('auto.fw.agentAttention.vault.jump', 'Jump to owned worktree')}
        />
        <VaultAction
          icon={History}
          label={translate(
            'auto.fw.agentAttention.vault.resume',
            'Resume · content + compatible target'
          )}
        />
        <VaultAction
          icon={FileClock}
          label={translate('auto.fw.agentAttention.vault.log', 'View log · local path available')}
        />
      </div>
    </section>
  )
}

function VaultAction(props: { icon: typeof History; label: string }): JSX.Element {
  const Icon = props.icon
  return (
    <span className="inline-flex shrink-0 items-center gap-1 rounded-md border border-border bg-card px-1.5 py-1 text-[11px] text-muted-foreground">
      <Icon className="size-2.5" aria-hidden />
      {props.label}
    </span>
  )
}
