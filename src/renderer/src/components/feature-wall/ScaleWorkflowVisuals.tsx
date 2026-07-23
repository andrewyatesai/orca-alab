import { useEffect, useState } from 'react'
import type { JSX } from 'react'
import {
  AlertTriangle,
  CalendarClock,
  CheckCircle2,
  Clock3,
  RotateCcw,
  Workflow
} from 'lucide-react'
import { translate } from '@/i18n/i18n'

type AutomationPhase = 'precheck' | 'failed' | 'rerun' | 'completed'

const AUTOMATION_PHASES: readonly AutomationPhase[] = ['precheck', 'failed', 'rerun', 'completed']

export function AutomationWorkflowVisual(props: { reducedMotion: boolean }): JSX.Element {
  const [animatedPhase, setAnimatedPhase] = useState<AutomationPhase>('precheck')
  const phase = props.reducedMotion ? 'completed' : animatedPhase
  const recoveryHistoryVisible = phase === 'rerun' || phase === 'completed'

  useEffect(() => {
    if (props.reducedMotion) {
      return
    }
    let index = 0
    const advance = (): void => {
      if (index >= AUTOMATION_PHASES.length - 1) {
        return
      }
      index += 1
      setAnimatedPhase(AUTOMATION_PHASES[index])
      if (index < AUTOMATION_PHASES.length - 1) {
        timeoutId = window.setTimeout(advance, 900)
      }
    }
    let timeoutId = window.setTimeout(advance, 900)
    return () => window.clearTimeout(timeoutId)
  }, [props.reducedMotion])

  return (
    <div
      className="w-full max-w-[620px] overflow-hidden rounded-xl border border-border bg-card shadow-xs"
      data-feature-wall-automation-phase={phase}
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
              'auto.components.feature.wall.ScaleWorkflowVisuals.d130000007',
              'Scheduled · Weekdays · Precheck enabled'
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
              icon={runPhaseCopy(phase).icon}
              label={runPhaseCopy(phase).label}
              detail={runPhaseCopy(phase).detail}
              state={runPhaseCopy(phase).state}
            />
            {recoveryHistoryVisible ? (
              <RunRow
                icon={AlertTriangle}
                label={translate(
                  'auto.components.feature.wall.ScaleWorkflowVisuals.d140000001',
                  'Previous attempt · Precheck failed'
                )}
                detail={translate(
                  'auto.components.feature.wall.ScaleWorkflowVisuals.d140000002',
                  'Target unavailable · run retained'
                )}
                state="failure"
              />
            ) : null}
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
              state="pending"
            />
          </div>
        </div>
      </div>
      <div className="grid grid-cols-4 border-t border-border bg-muted/20 text-[11px] text-muted-foreground">
        {AUTOMATION_PHASES.map((item, index) => (
          <div
            key={item}
            className={
              item === phase
                ? 'border-r border-border bg-accent px-2 py-2 text-center font-medium text-accent-foreground last:border-r-0'
                : 'border-r border-border px-2 py-2 text-center last:border-r-0'
            }
          >
            {index + 1} · {automationPhaseLabel(item)}
          </div>
        ))}
      </div>
    </div>
  )
}

function runPhaseCopy(phase: AutomationPhase): {
  icon: typeof CheckCircle2
  label: string
  detail: string
  state: 'success' | 'failure' | 'pending'
} {
  switch (phase) {
    case 'precheck':
      return {
        icon: Clock3,
        label: translate(
          'auto.components.feature.wall.ScaleWorkflowVisuals.d130000001',
          'Running precheck'
        ),
        detail: translate(
          'auto.components.feature.wall.ScaleWorkflowVisuals.d130000002',
          'Validating the target before dispatch'
        ),
        state: 'pending'
      }
    case 'failed':
      return {
        icon: AlertTriangle,
        label: translate(
          'auto.components.feature.wall.ScaleWorkflowVisuals.d130000003',
          'Precheck failed'
        ),
        detail: translate(
          'auto.components.feature.wall.ScaleWorkflowVisuals.d130000004',
          'Run preserved · fix and rerun'
        ),
        state: 'failure'
      }
    case 'rerun':
      return {
        icon: RotateCcw,
        label: translate(
          'auto.components.feature.wall.ScaleWorkflowVisuals.d130000005',
          'Rerunning'
        ),
        detail: translate(
          'auto.components.feature.wall.ScaleWorkflowVisuals.d130000006',
          'Same saved workflow · fresh workspace'
        ),
        state: 'pending'
      }
    case 'completed':
      return {
        icon: CheckCircle2,
        label: translate(
          'auto.components.feature.wall.ScaleWorkflowVisuals.d140000003',
          'Recovered on rerun'
        ),
        detail: translate(
          'auto.components.feature.wall.ScaleWorkflowVisuals.d140000004',
          'Fresh workspace · history and output retained'
        ),
        state: 'success'
      }
  }
}

function automationPhaseLabel(phase: AutomationPhase): string {
  switch (phase) {
    case 'precheck':
      return translate('auto.fw.automation.phase.precheck', 'Precheck')
    case 'failed':
      return translate('auto.fw.automation.phase.failed', 'Recover')
    case 'rerun':
      return translate('auto.fw.automation.phase.rerun', 'Rerun')
    case 'completed':
      return translate('auto.fw.automation.phase.completed', 'Inspect')
  }
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
  state: 'success' | 'failure' | 'pending'
}): JSX.Element {
  const Icon = props.icon
  return (
    <div className="flex items-start gap-2">
      <Icon
        className={
          props.state === 'success'
            ? 'mt-0.5 size-3.5 text-status-success'
            : props.state === 'failure'
              ? 'mt-0.5 size-3.5 text-destructive'
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
