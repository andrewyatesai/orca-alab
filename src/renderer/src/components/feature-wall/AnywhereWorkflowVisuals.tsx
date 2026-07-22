import type { JSX } from 'react'
import {
  ArrowRight,
  CheckCircle2,
  Laptop,
  MousePointer2,
  ShieldCheck,
  TerminalSquare
} from 'lucide-react'
import { translate } from '@/i18n/i18n'
export { RemoteMobileWorkflowVisual } from './RemoteMobileWorkflowVisual'

export function ComputerUseWorkflowVisual(): JSX.Element {
  return (
    <div
      className="grid w-full max-w-[640px] grid-cols-1 overflow-hidden rounded-xl border border-border bg-card shadow-xs sm:grid-cols-[240px_minmax(0,1fr)]"
      data-feature-wall-computer-use-visual="true"
      data-computer-use-flow="inspect-invoke-result"
      aria-hidden
    >
      <div className="border-b border-border bg-muted/20 p-4 sm:border-b-0 sm:border-r">
        <p className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
          {translate('auto.components.feature.wall.AnywhereWorkflowVisuals.e110000025', 'Access')}
        </p>
        <div className="mt-3 flex items-start gap-2.5 rounded-lg border border-border bg-background/70 p-3">
          <div className="flex size-8 shrink-0 items-center justify-center rounded-md border border-border bg-card">
            <ShieldCheck className="size-4 text-muted-foreground" />
          </div>
          <div className="min-w-0">
            <p className="text-xs font-medium">
              {translate(
                'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000011',
                'Capabilities checked'
              )}
            </p>
            <p className="mt-1 text-[11px] leading-snug text-muted-foreground">
              {translate(
                'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000026',
                'Native helper + advertised actions'
              )}
            </p>
            {/* Why: capability checks are cross-platform; this permission pair is macOS-only. */}
            <p className="mt-1.5 text-[11px] leading-snug text-muted-foreground">
              {translate(
                'auto.components.feature.wall.AnywhereWorkflowVisuals.e130000007',
                'macOS only · Accessibility + Screen Recording'
              )}
            </p>
          </div>
        </div>
        <div className="mt-5">
          <p className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
            {translate(
              'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000010',
              'Visible app scope'
            )}
          </p>
          <div className="mt-3 space-y-2">
            <AppRow
              icon={TerminalSquare}
              label={translate(
                'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000012',
                'Terminal'
              )}
              active
            />
            <AppRow
              icon={Laptop}
              label={translate(
                'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000013',
                'Editor'
              )}
            />
          </div>
        </div>
      </div>
      <div className="p-5">
        <p className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
          {translate(
            'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000014',
            'Accessibility snapshot'
          )}
        </p>
        <div
          className="mt-4 rounded-lg border border-border bg-muted/20 p-4 font-mono text-[11px] leading-6"
          data-computer-use-stage="inspect"
        >
          <p>
            {translate(
              'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000015',
              'window “Terminal”'
            )}
          </p>
          <p className="pl-4 text-muted-foreground">
            {translate(
              'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000016',
              'group “Session controls”'
            )}
          </p>
          <p className="rounded-sm bg-accent pl-8">
            {translate(
              'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000017',
              'button “Reconnect”'
            )}
          </p>
          <p className="pl-4 text-muted-foreground">
            {translate(
              'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000018',
              'text “Agent waiting”'
            )}
          </p>
        </div>
        <div className="mt-3 grid items-stretch gap-2 sm:grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)]">
          <div
            className="rounded-md border border-border bg-accent p-3"
            data-computer-use-stage="invoke"
            data-computer-use-action="reconnect"
          >
            <p className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
              {translate(
                'auto.components.feature.wall.AnywhereWorkflowVisuals.e150000001',
                'Invoke advertised action'
              )}
            </p>
            <p className="mt-1.5 flex items-center gap-1.5 font-mono text-[11px] font-medium">
              <MousePointer2 className="size-3.5 shrink-0 text-muted-foreground" />
              {translate(
                'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000017',
                'button “Reconnect”'
              )}
            </p>
          </div>
          <ArrowRight className="hidden size-3.5 self-center text-muted-foreground sm:block" />
          <div
            className="rounded-md border border-status-success-border bg-status-success-background p-3"
            data-computer-use-stage="result"
            data-computer-use-result="agent-connected"
          >
            <p className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
              {translate(
                'auto.components.feature.wall.AnywhereWorkflowVisuals.e150000002',
                'Visible result'
              )}
            </p>
            <p className="mt-1.5 flex items-center gap-1.5 text-[11px] font-medium text-status-success">
              <CheckCircle2 className="size-3.5 shrink-0" />
              {translate(
                'auto.components.feature.wall.AnywhereWorkflowVisuals.e150000003',
                'Agent connected'
              )}
            </p>
          </div>
        </div>
        <div className="mt-4 flex items-start gap-2.5 rounded-md border border-border p-3 text-xs">
          <MousePointer2 className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
          <div>
            <p className="font-medium">
              {translate(
                'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000027',
                'Advertised actions only'
              )}
            </p>
            <p className="mt-1 leading-relaxed text-muted-foreground">
              {translate(
                'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000019',
                'Inspect, click, type, scroll, and drag when the selected app advertises them.'
              )}
            </p>
          </div>
        </div>
      </div>
    </div>
  )
}

function AppRow(props: { icon: typeof Laptop; label: string; active?: boolean }): JSX.Element {
  const Icon = props.icon
  return (
    <div
      className={
        props.active
          ? 'flex items-center gap-2 rounded-md bg-accent p-2 text-xs'
          : 'flex items-center gap-2 rounded-md p-2 text-xs text-muted-foreground'
      }
    >
      <Icon className="size-3.5" />
      {props.label}
    </div>
  )
}
