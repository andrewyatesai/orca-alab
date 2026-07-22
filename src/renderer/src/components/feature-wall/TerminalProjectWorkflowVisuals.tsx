import type { JSX } from 'react'
import {
  ArrowRight,
  Download,
  FolderOpen,
  Laptop,
  MonitorUp,
  Plus,
  Server,
  SquareTerminal
} from 'lucide-react'
import { translate } from '@/i18n/i18n'
import { getFeatureWallTerminalShell } from './feature-wall-terminal-shell'

export function TerminalFirstWorkflowVisual(): JSX.Element {
  const shell = getFeatureWallTerminalShell()
  return (
    <div
      className="w-full max-w-[640px] overflow-hidden rounded-xl border border-border bg-card text-card-foreground shadow-xs"
      aria-hidden
    >
      <div className="flex h-10 items-center gap-2 border-b border-border bg-muted/40 px-3">
        <div className="flex items-center gap-1.5">
          <span className="size-2 rounded-full border border-border bg-background" />
          <span className="size-2 rounded-full border border-border bg-background" />
          <span className="size-2 rounded-full border border-border bg-background" />
        </div>
        <div className="ml-2 flex h-7 items-center gap-2 rounded-md border border-border bg-background px-3 text-xs font-medium">
          <SquareTerminal className="size-3.5 text-muted-foreground" />
          {translate(
            'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000001',
            'Shell'
          )}
        </div>
        <div className="flex h-7 items-center gap-2 rounded-md px-3 text-xs text-muted-foreground">
          {translate(
            'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000002',
            'Codex'
          )}
        </div>
        <Plus className="ml-auto size-3.5 text-muted-foreground" />
      </div>
      <div className="grid min-h-[300px] grid-cols-[minmax(0,1fr)_220px] font-mono text-xs">
        <div className="space-y-3 p-5">
          <p className="text-muted-foreground">{shell.banner}</p>
          <p>
            <span className="text-muted-foreground">{shell.prompt}</span>{' '}
            {translate(
              'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000004',
              'codex'
            )}
          </p>
          <div className="rounded-md border border-border bg-muted/30 p-3 font-sans">
            <div className="flex items-center gap-2 text-xs font-medium">
              <span className="size-1.5 rounded-full bg-status-success" />
              {translate(
                'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000005',
                'Agent session attached'
              )}
            </div>
            <p className="mt-1.5 text-xs leading-relaxed text-muted-foreground">
              {translate(
                'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000006',
                'Ready in the active workspace terminal.'
              )}
            </p>
          </div>
        </div>
        <div className="border-l border-border bg-muted/20 p-4 font-sans">
          <p className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
            {translate(
              'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000008',
              'Session'
            )}
          </p>
          <dl className="mt-4 space-y-3 text-xs">
            <div>
              <dt className="text-muted-foreground">
                {translate(
                  'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b130000003',
                  'Warm restart'
                )}
              </dt>
              <dd className="mt-1">
                {translate(
                  'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b130000004',
                  'Live process reattaches'
                )}
              </dd>
            </div>
            <div>
              <dt className="text-muted-foreground">
                {translate(
                  'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b130000001',
                  'Host reboot'
                )}
              </dt>
              <dd className="mt-1">
                {translate(
                  'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b130000002',
                  'Layout + scrollback restore'
                )}
              </dd>
            </div>
            <div>
              <dt className="text-muted-foreground">
                {translate(
                  'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000011',
                  'Layout'
                )}
              </dt>
              <dd className="mt-1">
                {translate(
                  'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000012',
                  'Tabs and nested splits'
                )}
              </dd>
            </div>
            <div>
              <dt className="text-muted-foreground">
                {translate(
                  'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000013',
                  'Engine'
                )}
              </dt>
              <dd className="mt-1 text-muted-foreground">
                {translate(
                  'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000014',
                  'aterm · Rust'
                )}
              </dd>
            </div>
          </dl>
        </div>
      </div>
    </div>
  )
}

export function AddProjectWorkflowVisual(): JSX.Element {
  const runtimeOptions = [
    {
      icon: Laptop,
      title: translate(
        'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000015',
        'This computer'
      ),
      detail: translate(
        'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b140000001',
        'Native host or WSL on Windows'
      )
    },
    {
      icon: Server,
      title: translate(
        'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000017',
        'SSH host'
      ),
      detail: translate(
        'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000018',
        'Connect to an existing machine over SSH'
      )
    },
    {
      icon: MonitorUp,
      title: translate(
        'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000019',
        'Paired Orca runtime'
      ),
      detail: translate(
        'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000020',
        'Pair with Orca running on another computer'
      )
    }
  ] as const
  const codebaseOptions = [
    {
      icon: FolderOpen,
      title: translate(
        'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000027',
        'Open existing folder'
      ),
      detail: translate(
        'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000028',
        'Keep the current checkout and branch'
      )
    },
    {
      icon: Download,
      title: translate(
        'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000029',
        'Clone repository'
      ),
      detail: translate(
        'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000030',
        'Clone into the chosen location'
      )
    },
    {
      icon: Plus,
      title: translate(
        'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000031',
        'Create project'
      ),
      detail: translate(
        'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000032',
        'Start from an empty folder'
      )
    }
  ] as const

  return (
    <div
      className="w-full max-w-[600px] rounded-xl border border-border bg-card p-5 shadow-xs"
      aria-hidden
    >
      <div className="mb-5">
        <p className="text-sm font-semibold">
          {translate(
            'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000021',
            'Add project'
          )}
        </p>
        <p className="mt-1 text-xs text-muted-foreground">
          {translate(
            'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000022',
            'Choose where work runs, then how the codebase gets there.'
          )}
        </p>
      </div>
      <div className="grid items-stretch gap-3 sm:grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)]">
        <ProjectStage
          step={translate(
            'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000023',
            '1 · Location'
          )}
          description={translate(
            'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000024',
            'Where project operations run'
          )}
          options={runtimeOptions}
        />
        <ArrowRight className="m-auto size-4 rotate-90 text-muted-foreground sm:rotate-0" />
        <ProjectStage
          step={translate(
            'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000025',
            '2 · Codebase'
          )}
          description={translate(
            'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000026',
            'How code gets there'
          )}
          options={codebaseOptions}
        />
      </div>
    </div>
  )
}

function ProjectStage(props: {
  step: string
  description: string
  options: readonly { icon: typeof Laptop; title: string; detail: string }[]
}): JSX.Element {
  return (
    <div className="rounded-lg border border-border bg-muted/20 p-3">
      <p className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
        {props.step}
      </p>
      <p className="mt-1 text-xs font-medium">{props.description}</p>
      <div className="mt-3 space-y-2">
        {props.options.map(({ icon: Icon, title, detail }) => (
          <div key={title} className="flex items-start gap-2.5 rounded-md bg-background/70 p-2.5">
            <div className="flex size-7 shrink-0 items-center justify-center rounded-md border border-border bg-card">
              <Icon className="size-3.5 text-muted-foreground" />
            </div>
            <div className="min-w-0">
              <p className="text-xs font-medium leading-tight">{title}</p>
              <p className="mt-1 text-[11px] leading-snug text-muted-foreground">{detail}</p>
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}
