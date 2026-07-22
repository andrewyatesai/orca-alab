import type { JSX } from 'react'
import { CalendarClock, CheckCircle2, Clock3, Workflow } from 'lucide-react'
import { translate } from '@/i18n/i18n'

export function AutomationWorkflowVisual(): JSX.Element {
  return (
    <div
      className="w-full max-w-[620px] overflow-hidden rounded-xl border border-border bg-card shadow-xs"
      aria-hidden
    >
      <div className="flex h-12 items-center border-b border-border px-4">
        <div>
          <p className="text-sm font-semibold">
            {translate(
              'auto.components.feature.wall.ScaleWorkflowVisuals.d110000001',
              'Automations'
            )}
          </p>
          <p className="text-xs text-muted-foreground">
            {translate(
              'auto.components.feature.wall.ScaleWorkflowVisuals.d110000002',
              'Saved work, on demand or on schedule'
            )}
          </p>
        </div>
        <div className="ml-auto rounded-full border border-border bg-muted/30 px-2.5 py-1 text-[11px] font-medium text-muted-foreground">
          {translate(
            'auto.components.feature.wall.ScaleWorkflowVisuals.d110000003',
            '2 saved workflows'
          )}
        </div>
      </div>
      <div className="grid grid-cols-[minmax(0,1fr)_230px]">
        <div className="space-y-3 border-r border-border p-4">
          <AutomationCard
            icon={CalendarClock}
            label={translate(
              'auto.components.feature.wall.ScaleWorkflowVisuals.d110000004',
              'Regression triage'
            )}
            detail={translate(
              'auto.components.feature.wall.ScaleWorkflowVisuals.d110000005',
              'Scheduled · Weekdays at 9:00 AM UTC'
            )}
          />
          <AutomationCard
            icon={Workflow}
            label={translate(
              'auto.components.feature.wall.ScaleWorkflowVisuals.d110000006',
              'Dependency review'
            )}
            detail={translate(
              'auto.components.feature.wall.ScaleWorkflowVisuals.d110000007',
              'Manual · Fresh workspace'
            )}
          />
        </div>
        <div className="p-4">
          <p className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
            {translate(
              'auto.components.feature.wall.ScaleWorkflowVisuals.d110000008',
              'Recent runs'
            )}
          </p>
          <div className="mt-4 space-y-4 text-xs">
            <RunRow
              icon={CheckCircle2}
              label={translate(
                'auto.components.feature.wall.ScaleWorkflowVisuals.d110000009',
                'Completed'
              )}
              detail={translate(
                'auto.components.feature.wall.ScaleWorkflowVisuals.d110000010',
                '2 minutes ago'
              )}
              success
            />
            <RunRow
              icon={Clock3}
              label={translate(
                'auto.components.feature.wall.ScaleWorkflowVisuals.d110000011',
                'Scheduled'
              )}
              detail={translate(
                'auto.components.feature.wall.ScaleWorkflowVisuals.d110000012',
                'Tomorrow at 9:00 AM UTC'
              )}
            />
          </div>
        </div>
      </div>
    </div>
  )
}

function AutomationCard(props: {
  icon: typeof CalendarClock
  label: string
  detail: string
}): JSX.Element {
  const Icon = props.icon
  return (
    <div className="rounded-lg border border-border p-3">
      <div className="flex items-start gap-3">
        <div className="flex size-8 shrink-0 items-center justify-center rounded-md border border-border bg-background text-muted-foreground">
          <Icon className="size-4" />
        </div>
        <div className="min-w-0">
          <p className="truncate text-xs font-medium">{props.label}</p>
          <p className="mt-1 text-xs leading-relaxed text-muted-foreground">{props.detail}</p>
        </div>
      </div>
    </div>
  )
}

function RunRow(props: {
  icon: typeof CheckCircle2
  label: string
  detail: string
  success?: boolean
}): JSX.Element {
  const Icon = props.icon
  return (
    <div className="flex items-start gap-2">
      <Icon
        className={
          props.success
            ? 'mt-0.5 size-3.5 text-status-success'
            : 'mt-0.5 size-3.5 text-muted-foreground'
        }
      />
      <div>
        <p className="font-medium">{props.label}</p>
        <p className="mt-0.5 text-muted-foreground">{props.detail}</p>
      </div>
    </div>
  )
}
