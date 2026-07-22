import type { JSX } from 'react'
import { CheckCircle2, GitPullRequest, MessageSquareText, PencilLine } from 'lucide-react'
import { translate } from '@/i18n/i18n'

export function ReviewShipWorkflowVisual(): JSX.Element {
  return (
    <div
      className="grid w-full max-w-[660px] grid-cols-[minmax(0,1fr)_220px] overflow-hidden rounded-xl border border-border bg-card shadow-xs"
      aria-hidden
    >
      <div className="border-r border-border">
        <div className="flex h-10 items-center border-b border-border px-4 text-xs font-medium">
          {translate(
            'auto.components.feature.wall.ReviewShipWorkflowVisual.c110000001',
            'src/terminal/session.ts'
          )}
          <span className="ml-auto text-muted-foreground">
            {translate(
              'auto.components.feature.wall.ReviewShipWorkflowVisual.c110000002',
              'Revision · 3 changes'
            )}
          </span>
        </div>
        <div className="space-y-1 p-4 font-mono text-[11px] leading-6">
          <DiffLine
            marker=" "
            text={translate(
              'auto.components.feature.wall.ReviewShipWorkflowVisual.c110000003',
              'export async function restoreSession(id: string) {'
            )}
          />
          <DiffLine
            marker="-"
            text={translate(
              'auto.components.feature.wall.ReviewShipWorkflowVisual.c110000004',
              'return reconnect(id)'
            )}
            removed
          />
          <DiffLine
            marker="+"
            text={translate(
              'auto.components.feature.wall.ReviewShipWorkflowVisual.c110000005',
              'const session = await reconnect(id)'
            )}
            added
          />
          <DiffLine
            marker="+"
            text={translate(
              'auto.components.feature.wall.ReviewShipWorkflowVisual.c110000006',
              'return verifyScrollback(session)'
            )}
            added
          />
          <DiffLine marker=" " text="}" />
        </div>
        <div className="mx-4 mb-4 rounded-md border border-border bg-muted/30 p-3 font-sans text-xs">
          <div className="flex items-center gap-2 font-medium">
            <MessageSquareText className="size-3.5 text-muted-foreground" />
            {translate(
              'auto.components.feature.wall.ReviewShipWorkflowVisual.c110000007',
              'Review note'
            )}
          </div>
          <p className="mt-1.5 leading-relaxed text-muted-foreground">
            {translate(
              'auto.components.feature.wall.ReviewShipWorkflowVisual.c110000008',
              'Preserve restored scrollback before the pane becomes visible.'
            )}
          </p>
        </div>
      </div>
      <div className="p-4">
        <p className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
          {translate(
            'auto.components.feature.wall.ReviewShipWorkflowVisual.c110000009',
            'Review to publish'
          )}
        </p>
        <div className="mt-3">
          <WorkflowStep
            icon={MessageSquareText}
            label={translate(
              'auto.components.feature.wall.ReviewShipWorkflowVisual.c110000010',
              'Annotate'
            )}
            detail={translate(
              'auto.components.feature.wall.ReviewShipWorkflowVisual.c110000007',
              'Review note'
            )}
          />
          <WorkflowStep
            icon={PencilLine}
            label={translate(
              'auto.components.feature.wall.ReviewShipWorkflowVisual.c110000011',
              'Revise'
            )}
            detail={translate(
              'auto.components.feature.wall.ReviewShipWorkflowVisual.c110000002',
              'Revision · 3 changes'
            )}
          />
          <WorkflowStep
            icon={CheckCircle2}
            label={translate(
              'auto.components.feature.wall.ReviewShipWorkflowVisual.c110000012',
              'Run checks'
            )}
            detail={translate(
              'auto.components.feature.wall.ReviewShipWorkflowVisual.c110000015',
              'Checks passing'
            )}
          />
          <WorkflowStep
            icon={GitPullRequest}
            label={translate(
              'auto.components.feature.wall.ReviewShipWorkflowVisual.c110000013',
              'Publish'
            )}
            detail={translate(
              'auto.components.feature.wall.ReviewShipWorkflowVisual.c110000014',
              'Pull / merge request'
            )}
            last
          />
        </div>
      </div>
    </div>
  )
}

function DiffLine(props: {
  marker: string
  text: string
  added?: boolean
  removed?: boolean
}): JSX.Element {
  const stateClass = props.added
    ? 'bg-status-success-background'
    : props.removed
      ? 'bg-destructive/10'
      : ''
  return (
    <div className={`flex gap-2 rounded-sm px-2 ${stateClass}`}>
      <span className="w-2 shrink-0 text-muted-foreground">{props.marker}</span>
      <span>{props.text}</span>
    </div>
  )
}

function WorkflowStep(props: {
  icon: typeof CheckCircle2
  label: string
  detail: string
  last?: boolean
}): JSX.Element {
  const Icon = props.icon
  return (
    <div className="relative flex gap-2.5 pb-3 last:pb-0">
      {!props.last ? <span className="absolute bottom-0 left-[11px] top-6 w-px bg-border" /> : null}
      <span className="relative z-10 inline-flex size-6 shrink-0 items-center justify-center rounded-full border border-border bg-card text-muted-foreground">
        <Icon className="size-3" />
      </span>
      <div className="min-w-0 pt-0.5">
        <p className="text-xs font-medium leading-none">{props.label}</p>
        <p className="mt-1 truncate text-[11px] text-muted-foreground">{props.detail}</p>
      </div>
    </div>
  )
}
