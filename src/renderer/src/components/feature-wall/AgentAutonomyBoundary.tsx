import type { JSX } from 'react'
import { ShieldAlert } from 'lucide-react'
import { translate } from '@/i18n/i18n'
import { cn } from '@/lib/utils'

export function AgentAutonomyBoundary(): JSX.Element {
  return (
    <footer className="grid gap-2 border-t border-border bg-muted/20 px-3.5 py-2 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center">
      <div className="flex min-w-0 items-start gap-2.5">
        <ShieldAlert className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
        <div>
          <p className="text-[11px] font-semibold">
            {translate('auto.fw.agentAttention.f130000038', 'Autonomy boundary')}
          </p>
          <p className="mt-0.5 text-[11px] leading-snug text-muted-foreground">
            {translate(
              'auto.fw.agentAttention.f130000039',
              'Worktrees isolate files, not the machine. Full autonomy uses your host access.'
            )}
          </p>
        </div>
      </div>
      <div className="grid grid-cols-2 rounded-lg border border-border bg-background p-0.5 text-[11px]">
        <AutonomyMode
          label={translate('auto.fw.agentAttention.f130000040', 'Manual')}
          detail={translate('auto.fw.agentAttention.f130000041', 'Asks before sensitive actions')}
          active
        />
        <AutonomyMode
          label={translate('auto.fw.agentAttention.f130000042', 'Full autonomy')}
          detail={translate('auto.fw.agentAttention.f130000043', 'Fewer approval prompts')}
        />
      </div>
    </footer>
  )
}

function AutonomyMode(props: { label: string; detail: string; active?: boolean }): JSX.Element {
  return (
    <div
      className={cn(
        'rounded-md px-2 py-1.5',
        props.active ? 'bg-accent text-accent-foreground' : 'text-muted-foreground'
      )}
    >
      <p className="font-semibold">{props.label}</p>
      <p className="mt-0.5 whitespace-nowrap text-[11px]">{props.detail}</p>
    </div>
  )
}
