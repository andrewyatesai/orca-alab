import type { JSX } from 'react'
import {
  ArrowRight,
  CloudCog,
  FileCode2,
  FolderGit2,
  GitBranch,
  Globe2,
  Laptop,
  RefreshCw,
  Server,
  ShieldCheck,
  Smartphone,
  TerminalSquare,
  WifiOff
} from 'lucide-react'
import { translate } from '@/i18n/i18n'

export function RemoteMobileWorkflowVisual(): JSX.Element {
  return (
    <div
      className="w-full max-w-[660px] rounded-xl border border-border bg-card p-4 shadow-xs"
      aria-hidden
    >
      <div className="grid items-stretch gap-2 sm:grid-cols-[1fr_auto_1fr_auto_1fr]">
        <Endpoint
          icon={Laptop}
          eyebrow={translate(
            'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000001',
            'Full client'
          )}
          title={translate(
            'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000002',
            'Orca desktop'
          )}
          detail={translate(
            'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000003',
            'Choose and guide work'
          )}
        />
        <Connection />
        {/* Why: the runtime owns desktop/mobile coordination; an SSH host is a
            downstream execution target and must not look like the mobile server. */}
        <Endpoint
          icon={Server}
          eyebrow={translate(
            'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000004',
            'Runtime'
          )}
          title={translate(
            'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000005',
            'Orca runtime'
          )}
          detail={translate(
            'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000029',
            'Coordination + session authority'
          )}
          emphasized
        />
        <Connection />
        <Endpoint
          icon={Smartphone}
          eyebrow={translate(
            'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000007',
            'Companion'
          )}
          title={translate(
            'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000008',
            'Orca Mobile'
          )}
          detail={translate(
            'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000009',
            'Monitor and reply'
          )}
          badge={translate(
            'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000010',
            'Beta'
          )}
        />
      </div>

      <div className="mt-3 grid gap-3 sm:grid-cols-[minmax(0,1.7fr)_minmax(150px,0.7fr)]">
        <section
          className="overflow-hidden rounded-lg border border-border bg-muted/20"
          data-feature-wall-ssh-continuity
        >
          <div className="flex items-center gap-2 border-b border-border px-3 py-2">
            <FolderGit2 className="size-3.5 text-muted-foreground" />
            <p className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
              {translate(
                'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000011',
                'Operational SSH path'
              )}
            </p>
          </div>
          <div className="grid items-center gap-2 p-3 sm:grid-cols-[minmax(0,1fr)_auto_minmax(0,0.9fr)]">
            <div className="rounded-md border border-border bg-background p-2.5">
              <p className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
                {translate(
                  'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000012',
                  'Optional downstream'
                )}
              </p>
              <p className="mt-1 text-xs font-medium">
                {translate(
                  'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000013',
                  'SSH project host'
                )}
              </p>
              <div className="mt-2 flex flex-wrap gap-1">
                <OperationChip
                  icon={TerminalSquare}
                  label={translate(
                    'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000026',
                    'Terminal'
                  )}
                />
                <OperationChip
                  icon={GitBranch}
                  label={translate(
                    'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000027',
                    'Git'
                  )}
                />
                <OperationChip
                  icon={FileCode2}
                  label={translate(
                    'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000028',
                    'Files'
                  )}
                />
              </div>
            </div>
            <ArrowRight className="size-3.5 text-muted-foreground" />
            <div className="rounded-md border border-border bg-background p-2.5">
              <div className="flex items-center gap-1.5">
                <Globe2 className="size-3.5 text-muted-foreground" />
                <p className="text-xs font-medium">
                  {translate(
                    'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000014',
                    'Forwarded port :3000'
                  )}
                </p>
              </div>
              <p className="mt-1 text-[11px] leading-relaxed text-muted-foreground">
                {translate(
                  'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000015',
                  'Preview in the Orca browser'
                )}
              </p>
            </div>
          </div>

          <div className="grid grid-cols-[1fr_auto_1fr_auto_1fr] items-center gap-1 border-t border-border bg-background/60 px-3 py-2">
            <ContinuityStep
              icon={WifiOff}
              label={translate(
                'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000016',
                'SSH disconnects'
              )}
            />
            <ArrowRight className="size-3 text-muted-foreground" />
            <ContinuityStep
              icon={RefreshCw}
              label={translate(
                'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000017',
                'Reconnect + recover'
              )}
            />
            <ArrowRight className="size-3 text-muted-foreground" />
            <ContinuityStep
              icon={ShieldCheck}
              label={translate(
                'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000018',
                'SSH owner retained'
              )}
            />
          </div>
          <p className="border-t border-border px-3 py-2 text-[11px] leading-relaxed text-muted-foreground">
            {translate(
              'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000019',
              'The workspace stays assigned to the remote host; saved port forwards return after reconnect, never as local execution.'
            )}
          </p>
        </section>

        <div className="grid gap-3">
          <DetailCard
            icon={CloudCog}
            eyebrow={translate(
              'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000020',
              'On demand'
            )}
            title={translate(
              'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000021',
              'Workspace environment'
            )}
            detail={translate(
              'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000022',
              'Provisioned from orca.yaml'
            )}
          />
          <DetailCard
            icon={Smartphone}
            eyebrow={translate(
              'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000023',
              'Mobile companion'
            )}
            title={translate(
              'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000024',
              'Notifications + follow-ups'
            )}
            detail={translate(
              'auto.components.feature.wall.RemoteMobileWorkflowVisual.b140000025',
              'Monitor · reply · Quick Commands'
            )}
          />
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
  emphasized?: boolean
  badge?: string
}): JSX.Element {
  const Icon = props.icon
  return (
    <div
      className={`flex min-h-24 flex-col items-center justify-center rounded-lg border p-2.5 text-center ${
        props.emphasized ? 'border-foreground/20 bg-accent' : 'border-border bg-muted/20'
      }`}
    >
      <div className="flex items-center justify-center gap-1.5">
        <p className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
          {props.eyebrow}
        </p>
        {props.badge ? (
          <span
            className="rounded-full border border-border bg-background px-1.5 py-0.5 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground"
            data-feature-wall-mobile-beta="true"
          >
            {props.badge}
          </span>
        ) : null}
      </div>
      <div className="mt-1.5 flex size-7 items-center justify-center rounded-md border border-border bg-background">
        <Icon className="size-3.5 text-muted-foreground" />
      </div>
      <p className="mt-1.5 text-xs font-medium">{props.title}</p>
      <p className="mt-0.5 text-[11px] leading-relaxed text-muted-foreground">{props.detail}</p>
    </div>
  )
}

function Connection(): JSX.Element {
  return <div className="m-auto h-5 w-px bg-border sm:h-px sm:w-5" aria-hidden />
}

function OperationChip(props: { icon: typeof Laptop; label: string }): JSX.Element {
  const Icon = props.icon
  return (
    <span className="inline-flex items-center gap-1 rounded-full border border-border bg-muted/30 px-1.5 py-0.5 text-[11px] text-muted-foreground">
      <Icon className="size-2.5" />
      {props.label}
    </span>
  )
}

function ContinuityStep(props: { icon: typeof Laptop; label: string }): JSX.Element {
  const Icon = props.icon
  return (
    <div className="flex min-w-0 flex-col items-center gap-1 text-center">
      <Icon className="size-3 text-muted-foreground" />
      <span className="text-[11px] leading-tight text-muted-foreground">{props.label}</span>
    </div>
  )
}

function DetailCard(props: {
  icon: typeof Laptop
  eyebrow: string
  title: string
  detail: string
}): JSX.Element {
  const Icon = props.icon
  return (
    <div className="rounded-lg border border-border bg-muted/20 p-3">
      <div className="flex items-start gap-2">
        <div className="flex size-7 shrink-0 items-center justify-center rounded-md border border-border bg-background">
          <Icon className="size-3.5 text-muted-foreground" />
        </div>
        <div className="min-w-0">
          <p className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
            {props.eyebrow}
          </p>
          <p className="mt-1 text-[11px] font-medium leading-snug">{props.title}</p>
          <p className="mt-1 text-[11px] leading-relaxed text-muted-foreground">{props.detail}</p>
        </div>
      </div>
    </div>
  )
}
