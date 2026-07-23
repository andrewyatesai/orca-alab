import type { JSX } from 'react'
import {
  ArrowRight,
  CheckCircle2,
  Gauge,
  Image,
  Plus,
  Search,
  Sparkles,
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
          <div
            className="grid gap-2 rounded-md border border-border bg-background/70 p-3 font-sans text-[11px] sm:grid-cols-2"
            data-feature-wall-terminal-capabilities
          >
            <TerminalCapability
              icon={Gauge}
              action={translate(
                'auto.fw.terminalFirst.capabilities.output.action',
                'Agent output flood'
              )}
              result={translate(
                'auto.fw.terminalFirst.capabilities.output.result',
                'Focused GPU/WebGL · CPU fallback · background QoS'
              )}
            />
            <TerminalCapability
              icon={Sparkles}
              action={translate('auto.fw.terminalFirst.capabilities.input.action', 'Remote typing')}
              result={translate(
                'auto.fw.terminalFirst.capabilities.input.result',
                'Predictive echo confirmed · effects available'
              )}
            />
            <TerminalCapability
              icon={Search}
              action={translate(
                'auto.fw.terminalFirst.capabilities.search.action',
                'Search “failed”'
              )}
              result={translate(
                'auto.fw.terminalFirst.capabilities.search.result',
                '12 full-scrollback matches'
              )}
            />
            <TerminalCapability
              icon={Image}
              action={translate(
                'auto.fw.terminalFirst.capabilities.media.action',
                'Agent build + artifacts'
              )}
              result={translate(
                'auto.fw.terminalFirst.capabilities.media.result',
                'Inline images · bundled compilers + solvers'
              )}
            />
          </div>
        </div>
        <div className="border-l border-border bg-muted/20 p-4 font-sans">
          <p className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
            {translate(
              'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000008',
              'Session'
            )}
          </p>
          <dl className="mt-3 space-y-2 text-xs">
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
          <div
            className="mt-3 border-t border-border pt-2 text-[11px]"
            data-feature-wall-quick-command-trust="project-review"
          >
            <p className="font-semibold">
              {translate(
                'auto.fw.terminalFirst.quickCommands.title',
                'Quick Commands · trust boundary'
              )}
            </p>
            <div className="mt-1 grid gap-1.5 leading-snug text-muted-foreground">
              <span className="flex items-center gap-1.5 rounded-md border border-border bg-background px-1.5 py-1">
                <span className="font-mono">1</span>
                {translate(
                  'auto.fw.terminalFirst.quickCommands.review',
                  'Review project command · checks'
                )}
                <ArrowRight className="ml-auto size-3" />
                <span className="font-medium text-foreground">
                  {translate(
                    'auto.fw.terminalFirst.quickCommands.trusted',
                    'Trusted for this content'
                  )}
                </span>
              </span>
              <span className="flex items-center gap-1.5 rounded-md border border-border bg-background px-1.5 py-1">
                <span className="font-mono">2</span>
                {translate('auto.fw.terminalFirst.quickCommands.run', 'Run checks')}
                <ArrowRight className="ml-auto size-3" />
                <CheckCircle2 className="size-3 text-status-success" />
                <span className="font-medium text-status-success">18/18</span>
              </span>
            </div>
            <p className="mt-1 leading-snug text-muted-foreground">
              {translate(
                'auto.fw.terminalFirst.quickCommands.project',
                'Project orca.yaml · inert until review; changes require re-review'
              )}
            </p>
            <p className="mt-1 leading-snug text-muted-foreground">
              {translate(
                'auto.fw.terminalFirst.quickCommands.user',
                'User-owned commands · run normally'
              )}
            </p>
          </div>
        </div>
      </div>
    </div>
  )
}

function TerminalCapability(props: {
  icon: typeof Gauge
  action: string
  result: string
}): JSX.Element {
  const Icon = props.icon
  return (
    <div className="flex min-w-0 items-start gap-2">
      <Icon className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
      <span className="min-w-0">
        <span className="block font-medium">{props.action}</span>
        <span className="block leading-snug text-muted-foreground">{props.result}</span>
      </span>
    </div>
  )
}

export { AddProjectWorkflowVisual } from './AddProjectWorkflowVisual'
