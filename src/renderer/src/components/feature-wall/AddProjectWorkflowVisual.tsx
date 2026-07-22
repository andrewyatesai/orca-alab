import type { JSX } from 'react'
import {
  ArrowDown,
  Check,
  CheckCircle2,
  Download,
  FolderGit2,
  FolderOpen,
  Laptop,
  MonitorUp,
  Plus,
  Server,
  SquareTerminal,
  UserRound,
  Wrench,
  type LucideIcon
} from 'lucide-react'
import { translate } from '@/i18n/i18n'
import { cn } from '@/lib/utils'

type ProjectChoice = {
  icon: LucideIcon
  title: string
  detail: string
  selected: boolean
}

export function AddProjectWorkflowVisual(): JSX.Element {
  const hostChoices: readonly ProjectChoice[] = [
    {
      icon: Laptop,
      title: translate(
        'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000015',
        'This computer'
      ),
      detail: translate(
        'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b140000001',
        'Native host or WSL on Windows'
      ),
      selected: false
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
      ),
      selected: true
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
      ),
      selected: false
    }
  ]
  const codebaseChoices: readonly ProjectChoice[] = [
    {
      icon: FolderOpen,
      title: translate(
        'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000027',
        'Open existing folder'
      ),
      detail: translate(
        'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000028',
        'Keep the current checkout and branch'
      ),
      selected: true
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
      ),
      selected: false
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
      ),
      selected: false
    }
  ]

  return (
    <div
      aria-hidden
      className="w-full max-w-[640px] rounded-xl border border-border bg-card p-4 text-card-foreground shadow-xs"
      data-add-project-workflow="action-progress-result"
    >
      <header className="flex items-start justify-between gap-3">
        <div>
          <p className="text-sm font-semibold">
            {translate(
              'auto.components.feature.wall.TerminalProjectWorkflowVisuals.b110000021',
              'Add project'
            )}
          </p>
          <p className="mt-1 text-xs text-muted-foreground">
            {translate('auto.fw.addProject.story', 'Illustrative action → progress → result')}
          </p>
        </div>
        <span className="rounded-full border border-border bg-muted px-2 py-1 text-[11px] font-medium text-muted-foreground">
          {translate('auto.fw.addProject.stage.action', 'Action')}
        </span>
      </header>

      <section className="mt-3 grid gap-3 sm:grid-cols-2" data-add-project-story-stage="action">
        <ChoiceGroup
          choices={hostChoices}
          label={translate('auto.fw.addProject.hostStep', '1 · Execution host')}
          selectedLabel={translate('auto.fw.addProject.selected', 'Selected')}
        />
        <ChoiceGroup
          choices={codebaseChoices}
          label={translate('auto.fw.addProject.codebaseStep', '2 · Codebase path')}
          selectedLabel={translate('auto.fw.addProject.selected', 'Selected')}
        />
      </section>

      <div className="my-2 flex items-center justify-center gap-2 text-[11px] font-medium text-muted-foreground">
        <UserRound className="size-3.5" />
        {translate('auto.fw.addProject.confirm', 'User confirms · Add this project')}
        <ArrowDown className="size-3.5" />
      </div>

      <section
        className="rounded-lg border border-border bg-muted/20 p-3"
        data-add-project-story-stage="progress"
      >
        <p className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
          {translate('auto.fw.addProject.stage.progress', 'Progress')}
        </p>
        <div className="mt-2 grid gap-1.5 sm:grid-cols-2">
          <ProgressBeat
            detail={translate(
              'auto.fw.addProject.project.branch',
              'Existing checkout · branch unchanged'
            )}
            icon={CheckCircle2}
            step="1"
            title={translate('auto.fw.addProject.project.added', 'Project added')}
          />
          <ProgressBeat
            detail={translate('auto.fw.addProject.workspace.detail', 'session-recovery')}
            icon={UserRound}
            step="2"
            title={translate(
              'auto.fw.addProject.workspace.action',
              'User action · Create workspace'
            )}
          />
          <ProgressBeat
            detail={translate(
              'auto.fw.addProject.worktree.detail',
              'Isolated worktree · branch feature/session-recovery'
            )}
            icon={FolderGit2}
            step="3"
            title={translate('auto.fw.addProject.worktree.title', 'New Git worktree')}
          />
          <ProgressBeat
            detail={translate(
              'auto.fw.addProject.setup.command',
              'orca.yaml scripts.setup · pnpm install'
            )}
            icon={Wrench}
            step="4"
            title={translate('auto.fw.addProject.setup.title', 'Approved shared setup runs')}
          />
        </div>
      </section>

      <section
        className="mt-2 flex items-center gap-3 rounded-lg border border-status-success-border bg-status-success-background p-3"
        data-add-project-story-stage="result"
      >
        <div className="flex size-8 shrink-0 items-center justify-center rounded-md border border-status-success-border bg-card text-status-success">
          <SquareTerminal className="size-4" />
        </div>
        <div className="min-w-0">
          <p className="text-xs font-semibold text-status-success">
            {translate('auto.fw.addProject.result.ready', 'Workspace ready')}
          </p>
          <p className="mt-0.5 text-[11px] text-muted-foreground">
            {translate(
              'auto.fw.addProject.result.detail',
              'Terminal open · repository setup complete'
            )}
          </p>
        </div>
        <Check className="ml-auto size-4 shrink-0 text-status-success" />
      </section>

      <p className="mt-2 text-[11px] leading-snug text-muted-foreground">
        {translate(
          'auto.fw.addProject.boundary',
          'Illustrative · Shared orca.yaml setup runs automatically for new Git worktrees only after the repository command content is approved; changes require re-review. Commands and duration vary by repository and host.'
        )}
      </p>
    </div>
  )
}

function ChoiceGroup(props: {
  choices: readonly ProjectChoice[]
  label: string
  selectedLabel: string
}): JSX.Element {
  return (
    <div className="rounded-lg border border-border bg-muted/20 p-2.5">
      <p className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
        {props.label}
      </p>
      <div className="mt-2 space-y-1">
        {props.choices.map(({ icon: Icon, title, detail, selected }) => (
          <div
            className={cn(
              'flex min-w-0 items-start gap-2 rounded-md px-2 py-1.5',
              selected ? 'bg-accent text-accent-foreground' : 'text-muted-foreground'
            )}
            data-selected={selected}
            key={title}
          >
            <Icon className="mt-0.5 size-3.5 shrink-0" />
            <span className="min-w-0">
              <span className="block text-[11px] font-medium leading-tight">{title}</span>
              <span className="mt-0.5 block text-[11px] leading-tight text-muted-foreground">
                {detail}
              </span>
            </span>
            {selected ? (
              <span className="ml-auto shrink-0 text-[11px] font-medium">
                {props.selectedLabel}
              </span>
            ) : null}
          </div>
        ))}
      </div>
    </div>
  )
}

function ProgressBeat(props: {
  detail: string
  icon: LucideIcon
  step: string
  title: string
}): JSX.Element {
  const Icon = props.icon
  return (
    <div
      className="flex min-w-0 items-start gap-2 rounded-md bg-background/70 p-2"
      data-add-project-progress-step={props.step}
    >
      <span className="font-mono text-[11px] text-muted-foreground">{props.step}</span>
      <Icon className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
      <span className="min-w-0">
        <span className="block text-[11px] font-medium leading-tight">{props.title}</span>
        <span className="mt-0.5 block text-[11px] leading-tight text-muted-foreground">
          {props.detail}
        </span>
      </span>
    </div>
  )
}
