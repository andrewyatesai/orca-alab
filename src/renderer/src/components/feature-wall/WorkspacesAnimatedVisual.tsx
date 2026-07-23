import { useEffect, useState } from 'react'
import type { JSX } from 'react'
import {
  Archive,
  ArrowRight,
  FolderGit2,
  GitBranch,
  GitCompareArrows,
  GitFork,
  Trophy,
  UserRoundCheck,
  Workflow,
  type LucideIcon
} from 'lucide-react'
import { AgentStateDot } from '@/components/AgentStateDot'
import { Badge } from '@/components/ui/badge'
import { translate } from '@/i18n/i18n'
import { cn } from '@/lib/utils'
import { ClaudeIcon, OpenCodeGoIcon } from '../status-bar/icons'
import { CodexInlineIcon } from './feature-tour-preview-glyphs'
import {
  WorkspaceBoardPreview,
  WorkspaceRaceBranchContext,
  WorkspaceRaceSectionLabel
} from './WorkspaceBoardPreview'

type AgentKind = 'claude' | 'codex' | 'opencode'

type Candidate = {
  kind: AgentKind
  name: string
  worktree: string
  checks: string
  additions: number
  deletions: number
  winner: boolean
}

const CANDIDATES: readonly Candidate[] = [
  {
    kind: 'codex',
    get name() {
      return translate('auto.components.feature.wall.WorkspacesAnimatedVisual.f110000007', 'Codex')
    },
    worktree: 'orca-yaml-codex',
    checks: '18/18',
    additions: 42,
    deletions: 8,
    winner: true
  },
  {
    kind: 'claude',
    get name() {
      return translate(
        'auto.components.feature.wall.WorkspacesAnimatedVisual.f110000008',
        'Claude Code'
      )
    },
    worktree: 'orca-yaml-claude',
    checks: '18/18',
    additions: 58,
    deletions: 13,
    winner: false
  },
  {
    kind: 'opencode',
    get name() {
      return translate(
        'auto.components.feature.wall.WorkspacesAnimatedVisual.f110000009',
        'OpenCode'
      )
    },
    worktree: 'orca-yaml-opencode',
    checks: '18/18',
    additions: 47,
    deletions: 15,
    winner: false
  }
]

const STORY_PHASES = ['source', 'fanout', 'working', 'compare', 'selected', 'cleanup'] as const
type StoryPhase = (typeof STORY_PHASES)[number]

const PHASE_DURATION_MS: Record<StoryPhase, number> = {
  source: 600,
  fanout: 650,
  working: 800,
  compare: 800,
  selected: 650,
  cleanup: 0
}
const STORY_SETTLE_MS = 3500

type CandidateState = 'ready' | 'working' | 'complete' | 'winner' | 'archived'

export function WorkspacesAnimatedVisual(props: { reducedMotion: boolean }): JSX.Element {
  const { reducedMotion } = props
  const [animatedPhaseIndex, setAnimatedPhaseIndex] = useState(0)
  // Why: deriving the reduced-motion result avoids mounting an animated frame and
  // repairing it in an effect, so the complete story is stable from first paint.
  const phase: StoryPhase = reducedMotion ? 'cleanup' : STORY_PHASES[animatedPhaseIndex]
  const outcomesVisible = phase === 'compare' || phase === 'selected' || phase === 'cleanup'

  useEffect(() => {
    if (reducedMotion || animatedPhaseIndex >= STORY_PHASES.length - 1) {
      return
    }
    const timeout = window.setTimeout(() => {
      setAnimatedPhaseIndex((current) => Math.min(current + 1, STORY_PHASES.length - 1))
    }, PHASE_DURATION_MS[phase])
    return () => window.clearTimeout(timeout)
  }, [animatedPhaseIndex, phase, reducedMotion])

  const stage = getStagePresentation(phase)
  const StageIcon = stage.icon

  return (
    <div
      className="overflow-hidden rounded-xl border border-border bg-card p-2.5 text-foreground"
      data-workspaces-story-phase={phase}
      data-feature-wall-story-loop="once"
      data-feature-wall-story-settle-ms={STORY_SETTLE_MS}
    >
      <div className="mb-2.5 grid grid-cols-[minmax(0,0.8fr)_auto_minmax(0,1.4fr)] items-stretch gap-2 rounded-lg border border-border bg-muted/30 p-2.5">
        <WorkspaceRaceBranchContext
          icon={GitBranch}
          label={translate(
            'auto.components.feature.wall.WorkspacesAnimatedVisual.f140000001',
            'Git project · base branch'
          )}
          primary="main"
        />
        <ArrowRight className="size-3.5 self-center text-muted-foreground" aria-hidden />
        <WorkspaceRaceBranchContext
          icon={FolderGit2}
          label={translate(
            'auto.components.feature.wall.WorkspacesAnimatedVisual.f110000002',
            'Isolated worktree + branch'
          )}
          primary="orca-yaml"
          secondary="feature/orca-yaml"
        />
      </div>

      <WorkspaceBoardPreview moved={phase !== 'source'} />

      <WorkspaceRaceSectionLabel />

      <div className="mb-2 flex items-center gap-2.5 rounded-lg border border-border bg-muted/30 px-2.5 py-2">
        <span className="inline-flex size-7 shrink-0 items-center justify-center rounded-md border border-border bg-card text-muted-foreground shadow-xs">
          <GitFork className="size-3.5" aria-hidden />
        </span>
        <div className="min-w-0 flex-1">
          <p className="text-[11px] font-semibold uppercase tracking-[0.05em] text-muted-foreground">
            {translate(
              'auto.components.feature.wall.WorkspacesAnimatedVisual.f110000010',
              'Same task, three candidates'
            )}
          </p>
          <p className="truncate text-[13px] font-semibold">
            {translate(
              'auto.components.feature.wall.WorkspacesAnimatedVisual.f110000004',
              'set up orca.yaml'
            )}
          </p>
        </div>
        <Badge variant="outline" className="h-5 px-1.5 text-[11px] text-muted-foreground">
          1 → 3
        </Badge>
      </div>

      <div className="grid grid-cols-[minmax(0,1fr)_48px_66px] gap-2 px-2 pb-1 text-[11px] font-semibold uppercase tracking-[0.05em] text-muted-foreground">
        <span>
          {translate(
            'auto.components.feature.wall.WorkspacesAnimatedVisual.f110000011',
            'Candidate'
          )}
        </span>
        <span className="text-right">
          {translate('auto.components.feature.wall.WorkspacesAnimatedVisual.f110000012', 'Checks')}
        </span>
        <span className="text-right">
          {translate('auto.components.feature.wall.WorkspacesAnimatedVisual.f110000013', 'Diff')}
        </span>
      </div>

      <div className="space-y-1.5">
        {CANDIDATES.map((candidate, index) => (
          <CandidateRow
            key={candidate.kind}
            candidate={candidate}
            index={index}
            phase={phase}
            outcomesVisible={outcomesVisible}
            reducedMotion={reducedMotion}
          />
        ))}
      </div>

      <div
        className="mt-2 flex min-h-12 items-center gap-2 rounded-lg border border-border bg-muted/30 px-2.5 py-1.5"
        data-winner-selection={stage.rationale ? 'user' : 'pending'}
        data-winner-rationale={stage.rationale ? 'checks-and-focused-diff' : undefined}
      >
        <StageIcon className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
        <span className="min-w-0 flex-1">
          <span className="block truncate text-[11px] font-medium">{stage.label}</span>
          {stage.rationale ? (
            <span className="mt-0.5 block text-[11px] leading-tight text-muted-foreground">
              {stage.rationale}
            </span>
          ) : null}
        </span>
        {stage.rationale ? (
          <span className="flex shrink-0 items-center gap-1 rounded-md border border-border bg-card px-1.5 py-1 font-mono text-[11px] text-muted-foreground">
            <UserRoundCheck className="size-3" aria-hidden />
            {translate('auto.fw.workspaces.winnerEvidence', 'Codex · 18/18 · +42 −8')}
          </span>
        ) : (
          <span className="shrink-0 font-mono text-[11px] text-muted-foreground">
            {translate(
              'auto.components.feature.wall.WorkspacesAnimatedVisual.f110000014',
              '1 task → 1 winner'
            )}
          </span>
        )}
      </div>
    </div>
  )
}

function CandidateRow(props: {
  candidate: Candidate
  index: number
  phase: StoryPhase
  outcomesVisible: boolean
  reducedMotion: boolean
}): JSX.Element {
  const { candidate, index, phase, outcomesVisible, reducedMotion } = props
  const state = getCandidateState(phase, candidate)
  const revealed = phase !== 'source'

  return (
    <div
      className={cn(
        'grid grid-cols-[minmax(0,1fr)_48px_66px] items-center gap-2 rounded-lg border border-border bg-card px-2 py-2',
        reducedMotion
          ? 'transition-none'
          : 'transition-[opacity,transform,background-color,border-color,box-shadow] duration-500',
        revealed ? 'translate-x-0 opacity-100' : 'translate-x-2 opacity-0',
        phase === 'compare' && 'bg-muted/30',
        state === 'winner' && 'border-ring bg-accent shadow-xs ring-1 ring-ring/30',
        state === 'archived' && 'bg-muted/20 opacity-50'
      )}
      data-agent-kind={candidate.kind}
      data-candidate-state={state}
      style={{ transitionDelay: reducedMotion ? undefined : `${index * 90}ms` }}
    >
      <div className="min-w-0">
        <div className="flex min-w-0 items-center gap-2">
          <span className="inline-flex size-4 shrink-0 items-center justify-center text-foreground">
            <AgentIcon kind={candidate.kind} />
          </span>
          <span className="truncate text-[12px] font-semibold">{candidate.name}</span>
          <CandidateStatus state={state} />
        </div>
        <p className="mt-0.5 truncate font-mono text-[11px] text-muted-foreground">
          {candidate.worktree}
        </p>
      </div>
      <span
        className={cn(
          'text-right font-mono text-[11px] font-medium transition-opacity duration-300',
          outcomesVisible ? 'opacity-100' : 'opacity-0'
        )}
      >
        {candidate.checks}
      </span>
      <span
        className={cn(
          'flex justify-end gap-1 font-mono text-[11px] transition-opacity duration-300',
          outcomesVisible ? 'opacity-100' : 'opacity-0'
        )}
      >
        <span className="[color:var(--git-decoration-added)]">+{candidate.additions}</span>
        <span className="[color:var(--git-decoration-deleted)]">−{candidate.deletions}</span>
      </span>
    </div>
  )
}

function CandidateStatus({ state }: { state: CandidateState }): JSX.Element {
  const label = getCandidateStateLabel(state)
  const Icon = state === 'archived' ? Archive : state === 'winner' ? Trophy : GitBranch
  return (
    <Badge
      variant={state === 'archived' ? 'secondary' : 'outline'}
      className={cn(
        'h-5 gap-1 px-1.5 text-[11px] text-muted-foreground',
        state === 'winner' && 'border-ring bg-background text-foreground'
      )}
    >
      {state === 'working' || state === 'complete' ? (
        <AgentStateDot state={state === 'working' ? 'working' : 'done'} size="sm" />
      ) : (
        <Icon className="size-2.5" aria-hidden />
      )}
      {label}
    </Badge>
  )
}

function AgentIcon({ kind }: { kind: AgentKind }): JSX.Element {
  if (kind === 'claude') {
    return <ClaudeIcon size={14} />
  }
  if (kind === 'codex') {
    return <CodexInlineIcon />
  }
  return <OpenCodeGoIcon size={14} />
}

function getCandidateState(phase: StoryPhase, candidate: Candidate): CandidateState {
  if (phase === 'source' || phase === 'fanout') {
    return 'ready'
  }
  if (phase === 'working') {
    return 'working'
  }
  if (phase === 'selected' || phase === 'cleanup') {
    return candidate.winner ? 'winner' : phase === 'cleanup' ? 'archived' : 'complete'
  }
  return 'complete'
}

function getCandidateStateLabel(state: CandidateState): string {
  switch (state) {
    case 'ready':
      return translate('auto.components.feature.wall.WorkspacesAnimatedVisual.f110000016', 'Ready')
    case 'working':
      return translate(
        'auto.components.feature.wall.WorkspacesAnimatedVisual.f110000017',
        'Working'
      )
    case 'complete':
      return translate(
        'auto.components.feature.wall.WorkspacesAnimatedVisual.f110000018',
        'Complete'
      )
    case 'winner':
      return translate('auto.components.feature.wall.WorkspacesAnimatedVisual.f110000019', 'Winner')
    case 'archived':
      return translate(
        'auto.components.feature.wall.WorkspacesAnimatedVisual.f110000020',
        'Archived'
      )
  }
}

function getStagePresentation(phase: StoryPhase): {
  icon: LucideIcon
  label: string
  rationale?: string
} {
  switch (phase) {
    case 'source':
      return {
        icon: GitBranch,
        label: translate(
          'auto.components.feature.wall.WorkspacesAnimatedVisual.f110000021',
          'Start with one task on main'
        )
      }
    case 'fanout':
      return {
        icon: GitFork,
        label: translate(
          'auto.components.feature.wall.WorkspacesAnimatedVisual.f110000022',
          'Fan out into three isolated worktrees'
        )
      }
    case 'working':
      return {
        icon: Workflow,
        label: translate(
          'auto.components.feature.wall.WorkspacesAnimatedVisual.f110000023',
          'Codex, Claude, and OpenCode work in parallel'
        )
      }
    case 'compare':
      return {
        icon: GitCompareArrows,
        label: translate(
          'auto.components.feature.wall.WorkspacesAnimatedVisual.f110000025',
          'Candidates finished · compare checks and diff outcomes'
        )
      }
    case 'selected':
      return {
        icon: UserRoundCheck,
        label: translate(
          'auto.components.feature.wall.WorkspacesAnimatedVisual.f110000026',
          'Codex chosen as the winner'
        ),
        rationale: translate(
          'auto.fw.workspaces.winnerRationale',
          'Human choice · all checks passed; smallest focused diff'
        )
      }
    case 'cleanup':
      return {
        icon: Archive,
        label: translate(
          'auto.components.feature.wall.WorkspacesAnimatedVisual.f110000027',
          'Winner kept · two alternatives archived'
        ),
        rationale: translate(
          'auto.fw.workspaces.winnerRationale',
          'Human choice · all checks passed; smallest focused diff'
        )
      }
  }
}
