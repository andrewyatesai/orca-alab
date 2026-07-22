import type { JSX } from 'react'
import {
  FolderGit2,
  Laptop,
  MousePointer2,
  Server,
  ShieldCheck,
  Smartphone,
  TerminalSquare
} from 'lucide-react'
import { translate } from '@/i18n/i18n'

export function RemoteMobileWorkflowVisual(): JSX.Element {
  return (
    <div
      className="w-full max-w-[660px] rounded-xl border border-border bg-card p-5 shadow-xs"
      aria-hidden
    >
      <div className="grid items-stretch gap-3 sm:grid-cols-[1fr_auto_1fr_auto_1fr]">
        <Endpoint
          icon={Laptop}
          eyebrow={translate(
            'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000020',
            'Full client'
          )}
          title={translate(
            'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000001',
            'Orca desktop'
          )}
          detail={translate(
            'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000002',
            'Choose and guide work'
          )}
        />
        <Connection />
        {/* Why: only the Mobile companion is beta; the surrounding runtime and
            SSH workflow must not inherit that availability label. */}
        <Endpoint
          icon={Server}
          eyebrow={translate(
            'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000021',
            'Runtime'
          )}
          title={translate(
            'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000003',
            'Orca runtime'
          )}
          detail={translate(
            'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000004',
            'This computer or a paired remote runtime'
          )}
          emphasized
        />
        <Connection />
        <Endpoint
          icon={Smartphone}
          eyebrow={translate(
            'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000028',
            'Companion'
          )}
          title={translate(
            'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000005',
            'Orca Mobile'
          )}
          detail={translate(
            'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000006',
            'Monitor and reply'
          )}
          badge={translate(
            'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000029',
            'Beta'
          )}
        />
      </div>
      <div className="flex flex-col items-center">
        <div className="h-4 w-px bg-border" />
        <div className="w-full max-w-[220px]">
          <Endpoint
            icon={FolderGit2}
            eyebrow={translate(
              'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000022',
              'Optional downstream'
            )}
            title={translate(
              'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000023',
              'SSH project host'
            )}
            detail={translate(
              'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000024',
              'Runs Git, files, and terminals'
            )}
            compact
          />
        </div>
      </div>
      <div className="mt-5 grid gap-2 text-xs sm:grid-cols-3">
        <Capability
          label={translate(
            'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000007',
            'Desktop source of truth'
          )}
        />
        <Capability
          label={translate(
            'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000008',
            'Agent attention'
          )}
        />
        <Capability
          label={translate(
            'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000009',
            'Safe reconnects'
          )}
        />
      </div>
    </div>
  )
}

export function ComputerUseWorkflowVisual(): JSX.Element {
  return (
    <div
      className="grid w-full max-w-[640px] grid-cols-1 overflow-hidden rounded-xl border border-border bg-card shadow-xs sm:grid-cols-[240px_minmax(0,1fr)]"
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
                'macOS permissions'
              )}
            </p>
            <p className="mt-1 text-[11px] leading-snug text-muted-foreground">
              {translate(
                'auto.components.feature.wall.AnywhereWorkflowVisuals.e110000026',
                'Accessibility + Screen Recording'
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
        <div className="mt-4 rounded-lg border border-border bg-muted/20 p-4 font-mono text-[11px] leading-6">
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

function Endpoint(props: {
  icon: typeof Laptop
  eyebrow: string
  title: string
  detail: string
  compact?: boolean
  emphasized?: boolean
  badge?: string
}): JSX.Element {
  const Icon = props.icon
  return (
    <div
      className={`flex flex-col items-center justify-center rounded-lg border p-3 text-center ${
        props.compact ? 'min-h-20' : 'min-h-28'
      } ${props.emphasized ? 'border-foreground/20 bg-accent' : 'border-border bg-muted/20'}`}
    >
      <div className="flex items-center justify-center gap-1.5">
        <p className="text-[10px] font-semibold uppercase tracking-wide text-muted-foreground">
          {props.eyebrow}
        </p>
        {props.badge ? (
          <span
            className="rounded-full border border-border bg-background px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-wide text-muted-foreground"
            data-feature-wall-mobile-beta="true"
          >
            {props.badge}
          </span>
        ) : null}
      </div>
      <div className="mt-2 flex size-8 items-center justify-center rounded-md border border-border bg-background">
        <Icon className="size-4 text-muted-foreground" />
      </div>
      <p className="mt-2 text-xs font-medium">{props.title}</p>
      <p className="mt-1 text-[11px] leading-relaxed text-muted-foreground">{props.detail}</p>
    </div>
  )
}

function Connection(): JSX.Element {
  return <div className="m-auto h-6 w-px bg-border sm:h-px sm:w-6" aria-hidden />
}

function Capability(props: { label: string }): JSX.Element {
  return (
    <div className="rounded-md border border-border bg-muted/20 px-3 py-2 text-center text-muted-foreground">
      {props.label}
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
