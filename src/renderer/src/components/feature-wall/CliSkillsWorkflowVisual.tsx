import { useEffect, useState } from 'react'
import type { JSX } from 'react'
import {
  BookOpenCheck,
  Bot,
  CheckCircle2,
  FileJson2,
  FolderGit2,
  Globe2,
  Server,
  SquareTerminal
} from 'lucide-react'
import { translate } from '@/i18n/i18n'
import { cn } from '@/lib/utils'

type CliPhase = 'discover' | 'create' | 'operate' | 'verify'

const PHASES: readonly CliPhase[] = ['discover', 'create', 'operate', 'verify']
const PHASE_MS = 900
const STORY_SETTLE_MS = PHASE_MS * (PHASES.length - 1)

export function CliSkillsWorkflowVisual(props: { reducedMotion: boolean }): JSX.Element {
  const [animatedPhase, setAnimatedPhase] = useState<CliPhase>('discover')
  const phase = props.reducedMotion ? 'verify' : animatedPhase
  const phaseIndex = PHASES.indexOf(phase)

  useEffect(() => {
    if (props.reducedMotion) {
      return
    }
    const nextPhase = PHASES[PHASES.indexOf(animatedPhase) + 1]
    if (!nextPhase) {
      return
    }
    const timeout = window.setTimeout(() => setAnimatedPhase(nextPhase), PHASE_MS)
    return () => window.clearTimeout(timeout)
  }, [animatedPhase, props.reducedMotion])

  return (
    <div
      className="w-full overflow-hidden rounded-xl border border-border bg-card shadow-xs"
      data-feature-wall-cli-phase={phase}
      data-feature-wall-story-loop="once"
      data-feature-wall-story-settle-ms={STORY_SETTLE_MS}
      aria-hidden
    >
      <div className="flex h-11 items-center gap-2 border-b border-border bg-muted/30 px-3">
        <Bot className="size-4 text-muted-foreground" />
        <span className="text-xs font-medium">
          {translate(
            'auto.components.feature.wall.CliSkillsWorkflowVisual.j130000001',
            'Agent control loop'
          )}
        </span>
        <div className="ml-auto flex items-center gap-1 text-[11px] text-muted-foreground">
          <SourceChip
            label={translate(
              'auto.components.feature.wall.CliSkillsWorkflowVisual.j130000009',
              'Bundled'
            )}
          />
          <SourceChip
            label={translate(
              'auto.components.feature.wall.CliSkillsWorkflowVisual.j130000010',
              'Repository'
            )}
          />
          <SourceChip
            label={translate(
              'auto.components.feature.wall.CliSkillsWorkflowVisual.j130000011',
              'Personal'
            )}
          />
          <SourceChip
            label={translate(
              'auto.components.feature.wall.CliSkillsWorkflowVisual.j130000012',
              'Plugin'
            )}
          />
        </div>
      </div>

      <div className="grid min-h-[310px] grid-cols-[minmax(0,1.2fr)_minmax(240px,0.8fr)]">
        <TerminalTranscript phaseIndex={phaseIndex} />
        <OrcaOutcomePanel phase={phase} phaseIndex={phaseIndex} />
      </div>

      <div className="grid grid-cols-4 border-t border-border bg-muted/20">
        <PhaseLabel
          index={0}
          current={phaseIndex}
          label={translate(
            'auto.components.feature.wall.CliSkillsWorkflowVisual.j130000014',
            'Discover'
          )}
        />
        <PhaseLabel
          index={1}
          current={phaseIndex}
          label={translate(
            'auto.components.feature.wall.CliSkillsWorkflowVisual.j130000015',
            'Create'
          )}
        />
        <PhaseLabel
          index={2}
          current={phaseIndex}
          label={translate(
            'auto.components.feature.wall.CliSkillsWorkflowVisual.j130000016',
            'Operate'
          )}
        />
        <PhaseLabel
          index={3}
          current={phaseIndex}
          label={translate(
            'auto.components.feature.wall.CliSkillsWorkflowVisual.j130000017',
            'Verify'
          )}
        />
      </div>
    </div>
  )
}

function SourceChip(props: { label: string }): JSX.Element {
  return (
    <span className="rounded-full border border-border bg-background px-1.5 py-0.5">
      {props.label}
    </span>
  )
}

function TerminalTranscript(props: { phaseIndex: number }): JSX.Element {
  return (
    <div className="border-r border-border bg-[var(--editor-surface)] p-4 font-mono text-[11px] leading-5">
      <div className="flex items-center gap-2 font-sans text-[11px] text-muted-foreground">
        <SquareTerminal className="size-3.5" />
        <span>
          {translate(
            'auto.components.feature.wall.CliSkillsWorkflowVisual.j130000002',
            'Agent terminal · SSH build host'
          )}
        </span>
      </div>
      <p className="mb-2 truncate pl-[22px] font-sans text-[11px] text-muted-foreground">
        <span className="font-mono text-foreground">
          {translate('auto.fw.cliSkills.commandName', 'orca')}
        </span>{' '}
        ·{' '}
        {translate(
          'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000017',
          'SSH host'
        )}
      </p>
      <CommandLine
        visible={props.phaseIndex >= 0}
        command="orca skills get orca-cli --full --json"
        result='{"name":"orca-cli","full":true,"markdown":"# Orca CLI\\n…"}'
      />
      <CommandLine
        visible={props.phaseIndex >= 1}
        command="orca worktree create --name login-race --agent codex --json"
        result='result.worktree.displayName = "login-race"'
      />
      <CommandLine
        visible={props.phaseIndex >= 2}
        command="orca snapshot --json"
        result='result.refs[0] = {"ref":"@e3","role":"button","name":"Run checks"}'
      />
      <CommandLine
        visible={props.phaseIndex >= 3}
        command="orca click --element @e3 --json"
        result='result = {"clicked":"@e3"}'
      />
      <CommandLine
        visible={props.phaseIndex >= 3}
        command="orca snapshot --json"
        result='result.refs[0] = {"ref":"@e4","role":"status","name":"Checks passed"}'
      />
    </div>
  )
}

function CommandLine(props: { visible: boolean; command: string; result: string }): JSX.Element {
  return (
    <div
      className={cn(
        'mb-2 transition-[opacity,transform] duration-300 motion-reduce:transition-none',
        props.visible ? 'translate-y-0 opacity-100' : 'translate-y-1 opacity-20'
      )}
    >
      <p className="truncate">
        <span className="mr-1 text-muted-foreground">›</span>
        {props.command}
      </p>
      <p className="truncate pl-3 text-muted-foreground">{props.result}</p>
    </div>
  )
}

function OrcaOutcomePanel(props: { phase: CliPhase; phaseIndex: number }): JSX.Element {
  return (
    <div className="p-4">
      <div className="flex items-center gap-2">
        <Server className="size-3.5 text-muted-foreground" />
        <p className="text-[11px] font-semibold uppercase tracking-[0.05em] text-muted-foreground">
          {translate(
            'auto.components.feature.wall.CliSkillsWorkflowVisual.j130000003',
            'Example Orca state'
          )}
        </p>
      </div>
      <div className="mt-3 space-y-2">
        <OutcomeRow
          icon={BookOpenCheck}
          label={translate(
            'auto.components.feature.wall.CliSkillsWorkflowVisual.j130000004',
            'Version-matched skill loaded'
          )}
          detail={`orca-cli · ${translate(
            'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000017',
            'SSH host'
          )}`}
          done={props.phaseIndex >= 0}
          active={props.phase === 'discover'}
        />
        <OutcomeRow
          icon={FolderGit2}
          label={translate(
            'auto.components.feature.wall.CliSkillsWorkflowVisual.j130000005',
            'Workspace appeared'
          )}
          detail="login-race · Codex"
          done={props.phaseIndex >= 1}
          active={props.phase === 'create'}
        />
        <OutcomeRow
          icon={Globe2}
          label={translate(
            'auto.components.feature.wall.CliSkillsWorkflowVisual.j130000006',
            'Browser element targeted'
          )}
          detail={`@e3 · ${translate(
            'auto.components.feature.wall.CliSkillsWorkflowVisual.j140000020',
            'button'
          )}`}
          done={props.phaseIndex >= 2}
          active={props.phase === 'operate'}
        />
        <OutcomeRow
          icon={CheckCircle2}
          label={translate(
            'auto.components.feature.wall.CliSkillsWorkflowVisual.j130000007',
            'Result verified'
          )}
          detail={translate(
            'auto.components.feature.wall.CliSkillsWorkflowVisual.j130000019',
            'Post-click snapshot · status observed'
          )}
          done={props.phaseIndex >= 3}
          active={props.phase === 'verify'}
        />
      </div>
      <div className="mt-3 flex items-start gap-2 rounded-md border border-border bg-muted/25 p-2 text-[11px] text-muted-foreground">
        <FileJson2 className="mt-0.5 size-3 shrink-0" />
        <span>
          {translate(
            'auto.components.feature.wall.CliSkillsWorkflowVisual.j130000008',
            'JSON output keeps agent actions inspectable and scriptable.'
          )}
        </span>
      </div>
    </div>
  )
}

function OutcomeRow(props: {
  icon: typeof CheckCircle2
  label: string
  detail: string
  done: boolean
  active: boolean
}): JSX.Element {
  const Icon = props.icon
  return (
    <div
      className={cn(
        'grid grid-cols-[28px_minmax(0,1fr)_14px] items-center gap-2 rounded-lg border p-2.5 transition-colors',
        props.active ? 'border-ring bg-accent' : 'border-border bg-background/70',
        !props.done && 'opacity-40'
      )}
    >
      <span className="flex size-7 items-center justify-center rounded-md border border-border bg-card">
        <Icon className="size-3.5 text-muted-foreground" />
      </span>
      <span className="min-w-0">
        <span className="block truncate text-[11px] font-medium">{props.label}</span>
        <span className="block truncate font-mono text-[11px] text-muted-foreground">
          {props.detail}
        </span>
      </span>
      {props.done ? <CheckCircle2 className="size-3.5 text-status-success" /> : null}
    </div>
  )
}

function PhaseLabel(props: { index: number; current: number; label: string }): JSX.Element {
  return (
    <div
      className={cn(
        'border-r border-border px-2 py-2 text-center text-[11px] last:border-r-0',
        props.index === props.current
          ? 'bg-accent font-medium text-accent-foreground'
          : 'text-muted-foreground'
      )}
    >
      {props.index + 1} · {props.label}
    </div>
  )
}
