import type { JSX } from 'react'
import { ArrowRight, CheckCircle2, Columns3, UserRound, type LucideIcon } from 'lucide-react'
import { translate } from '@/i18n/i18n'
import { cn } from '@/lib/utils'

export function WorkspaceBoardPreview(props: { moved: boolean }): JSX.Element {
  const todoLabel = translate('auto.fw.workspaces.board.todo', 'Todo')
  const progressLabel = translate('auto.fw.workspaces.board.progress', 'In progress')
  const lanes = [
    [todoLabel, props.moved ? '1' : '2'],
    [progressLabel, props.moved ? '4' : '3'],
    [translate('auto.fw.workspaces.board.review', 'In review'), '1'],
    [translate('auto.fw.workspaces.board.done', 'Done'), '4']
  ] as const
  return (
    <section
      className="mb-2.5 rounded-lg border border-border bg-muted/20 px-2.5 py-2"
      data-feature-wall-workspace-board="status-lanes"
    >
      <div className="flex min-w-0 items-center gap-2">
        <Columns3 className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
        <span className="text-[11px] font-semibold uppercase tracking-[0.05em] text-muted-foreground">
          {translate('auto.fw.workspaces.board.title', 'Workspace board')}
        </span>
        <span className="ml-auto truncate text-[11px] text-muted-foreground">
          {translate(
            'auto.fw.workspaces.board.boundary',
            'Existing workspaces · user-assigned status'
          )}
        </span>
      </div>
      <div className="mt-1.5 grid grid-cols-4 gap-1.5">
        {lanes.map(([label, count], index) => (
          <div
            key={label}
            className={cn(
              'min-w-0 rounded-md border border-border bg-card px-1.5 py-1',
              index === (props.moved ? 1 : 0) && 'border-ring bg-accent ring-1 ring-ring/20'
            )}
          >
            <div className="flex items-center gap-1 text-[11px] font-medium">
              <span className="size-1.5 shrink-0 rounded-full bg-muted-foreground/60" />
              <span className="truncate">{label}</span>
              <span className="ml-auto font-mono text-muted-foreground">{count}</span>
            </div>
          </div>
        ))}
      </div>
      <div
        className="mt-1.5 grid grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)_auto] items-center gap-1.5 text-[11px]"
        data-board-move-owner="user"
        data-board-move-state={props.moved ? 'complete' : 'ready'}
      >
        <span
          className={cn(
            'flex min-w-0 items-center gap-1 rounded-md border border-border bg-card px-1.5 py-1 text-muted-foreground',
            !props.moved && 'border-ring bg-accent text-accent-foreground ring-1 ring-ring/20'
          )}
        >
          <UserRound className="size-2.5 shrink-0" aria-hidden />
          <span className="truncate">{todoLabel}</span>
        </span>
        <ArrowRight className="size-3 text-muted-foreground" aria-hidden />
        <span
          className={cn(
            'flex min-w-0 items-center gap-1 rounded-md border border-border bg-card px-1.5 py-1 text-muted-foreground',
            props.moved && 'border-ring bg-accent text-accent-foreground ring-1 ring-ring/20'
          )}
        >
          <CheckCircle2 className="size-2.5 shrink-0" aria-hidden />
          <span className="truncate">{progressLabel}</span>
        </span>
        <span className="truncate font-mono text-muted-foreground">
          {translate(
            'auto.components.feature.wall.WorkspacesAnimatedVisual.f110000004',
            'set up orca.yaml'
          )}
        </span>
      </div>
    </section>
  )
}

export function WorkspaceRaceSectionLabel(): JSX.Element {
  return (
    <div className="mb-2 flex items-center gap-2 px-1">
      <span className="shrink-0 text-[11px] font-semibold uppercase tracking-[0.05em] text-muted-foreground">
        {translate(
          'auto.components.feature.wall.WorkspacesAnimatedVisual.f110000003',
          'Agent activity in isolated workspaces'
        )}
      </span>
      <span className="h-px min-w-0 flex-1 bg-border" />
    </div>
  )
}

export function WorkspaceRaceBranchContext(props: {
  icon: LucideIcon
  label: string
  primary: string
  secondary?: string
}): JSX.Element {
  const Icon = props.icon
  return (
    <div className="flex min-w-0 items-start gap-2">
      <span className="inline-flex size-6 shrink-0 items-center justify-center rounded-md border border-border bg-card text-muted-foreground">
        <Icon className="size-3" aria-hidden />
      </span>
      <div className="min-w-0">
        <p className="text-[11px] font-semibold uppercase tracking-[0.05em] text-muted-foreground">
          {props.label}
        </p>
        <p className="truncate font-mono text-[11px] leading-4 text-foreground">{props.primary}</p>
        {props.secondary ? (
          <p className="truncate font-mono text-[11px] leading-4 text-muted-foreground">
            {props.secondary}
          </p>
        ) : null}
      </div>
    </div>
  )
}
