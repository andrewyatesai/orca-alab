import { useEffect, useState } from 'react'
import type { JSX } from 'react'
import {
  Archive,
  ArrowRight,
  Check,
  CheckCircle2,
  CircleAlert,
  GitCommitHorizontal,
  GitCompareArrows,
  GitMerge,
  GitPullRequest,
  ListChecks,
  MessageSquareText,
  RotateCcw,
  Send,
  ShieldCheck,
  Upload
} from 'lucide-react'
import { translate } from '@/i18n/i18n'
import { cn } from '@/lib/utils'

type StoryPhase = 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9
type StoryKind =
  | 'compare'
  | 'annotate'
  | 'decision'
  | 'blocked'
  | 'retry'
  | 'passed'
  | 'rereview'
  | 'stage'
  | 'confirm'
  | 'archive'
type StoryItem = readonly [icon: typeof Archive, kind: StoryKind, title: string, detail: string]

const STORY: readonly StoryItem[] = [
  [GitCompareArrows, 'compare', 'Compare candidates', 'Inspect parallel diffs before choosing.'],
  [MessageSquareText, 'annotate', 'Annotate + send revision', 'Attach feedback; request a pass.'],
  [ShieldCheck, 'decision', 'Review decision', 'Recheck the revision before checks or Git writes.'],
  [CircleAlert, 'blocked', 'Failed check / conflict', 'Keep failed checks and conflicts visible.'],
  [RotateCcw, 'retry', 'Return, resolve + retry', 'Resolve in the same workspace, then retry.'],
  [CheckCircle2, 'passed', 'Checks pass', 'Use refreshed checks for the human decision.'],
  [
    ShieldCheck,
    'rereview',
    'Re-review resolved diff',
    'Human approval covers the recovered diff before staging or writes.'
  ],
  [ListChecks, 'stage', 'Stage focused hunk', 'Stage only the reviewed hunk before Git writes.'],
  [
    Upload,
    'confirm',
    'Confirm commit + push, then PR / MR',
    'Keep Git writes separate from PR / MR creation.'
  ],
  [Archive, 'archive', 'Workspace archived', 'Move completed work out of the active list.']
]
const PHASE_DURATION_MS = 400
const PANEL_MOTION_MS = 500
const REVIEW_MOTION_DURATION_MS = (STORY.length - 1) * PHASE_DURATION_MS + PANEL_MOTION_MS

const copy = (key: string, fallback: string): string =>
  translate(`auto.components.feature.wall.ReviewShipWorkflowVisual.${key}`, fallback)

export function ReviewShipWorkflowVisual(props: { reducedMotion: boolean }): JSX.Element {
  const [animatedPhase, setAnimatedPhase] = useState<StoryPhase>(0)

  useEffect(() => {
    if (props.reducedMotion) {
      return
    }
    let phase: StoryPhase = 0
    let timeoutId = 0
    const advance = (): void => {
      const nextPhase = phase + 1
      if (nextPhase >= STORY.length) {
        return
      }
      phase = nextPhase as StoryPhase
      setAnimatedPhase(phase)
      if (nextPhase < STORY.length - 1) {
        timeoutId = window.setTimeout(advance, PHASE_DURATION_MS)
      }
    }
    timeoutId = window.setTimeout(advance, PHASE_DURATION_MS)
    return () => window.clearTimeout(timeoutId)
  }, [props.reducedMotion])

  // Why: the static preference should preserve the complete recovery and
  // approval narrative, not freeze on the opening comparison.
  const phase: StoryPhase = props.reducedMotion ? 9 : animatedPhase
  const activeItem = STORY[phase]

  return (
    <div
      className="w-full max-w-[660px] overflow-hidden rounded-xl border border-border bg-card text-card-foreground shadow-xs"
      data-feature-wall-review-ship-visual
      data-story-phase={activeItem[1]}
      data-reduced-motion={props.reducedMotion ? 'true' : 'false'}
      data-animation-duration-ms={REVIEW_MOTION_DURATION_MS}
      aria-hidden
    >
      <header className="flex h-12 items-center gap-3 border-b border-border px-4">
        <span className="flex size-7 items-center justify-center rounded-md border border-border bg-muted/40">
          <GitCompareArrows className="size-3.5 text-muted-foreground" />
        </span>
        <div>
          <p className="text-xs font-semibold">{copy('header.title', 'Review and ship')}</p>
          <p className="text-[11px] text-muted-foreground">
            {copy('header.detail', 'Compare, recover, confirm, then close the workspace loop')}
          </p>
        </div>
      </header>

      <div className="grid sm:grid-cols-[minmax(0,1fr)_224px]">
        <section className="border-b border-border p-4 sm:border-b-0 sm:border-r">
          <CandidateComparison focused={phase > 0} />
          <div className="mt-3 min-h-[190px] overflow-hidden rounded-lg border border-border bg-muted/20">
            <StoryPanel
              key={activeItem[1]}
              item={activeItem}
              index={phase}
              reducedMotion={props.reducedMotion}
            />
          </div>
        </section>

        <aside className="bg-muted/10 p-3.5">
          <div className="mb-3 flex items-center justify-between text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
            <span>{copy('timeline.title', 'Review path')}</span>
            <span>
              {phase + 1}/{STORY.length}
            </span>
          </div>
          {STORY.map((item, index) => (
            <TimelineStep
              key={item[1]}
              item={item}
              index={index}
              phase={phase}
              last={index === STORY.length - 1}
            />
          ))}
        </aside>
      </div>

      <footer className="flex items-start gap-2.5 border-t border-border bg-muted/20 px-4 py-3">
        <ShieldCheck className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
        <p className="text-[11px] leading-relaxed text-muted-foreground">
          {copy(
            'footer.disclaimer',
            'Storyboard only: checks, conflicts, and provider results depend on the repository and Git host.'
          )}
        </p>
      </footer>
    </div>
  )
}

function CandidateComparison(props: { focused: boolean }): JSX.Element {
  return (
    <div>
      <p className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
        {copy('candidate.title', 'Candidate comparison')}
      </p>
      <div className="mt-2 grid grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] items-center gap-2">
        <CandidateCard
          label={copy('candidate.a', 'Candidate A')}
          files={copy('candidate.aFiles', '3 files')}
          diff={['+48', '−12']}
          focused={props.focused}
        />
        <GitCompareArrows className="size-3.5 text-muted-foreground" />
        <CandidateCard
          label={copy('candidate.b', 'Candidate B')}
          files={copy('candidate.bFiles', '5 files')}
          diff={['+73', '−19']}
          focused={false}
        />
      </div>
    </div>
  )
}

function CandidateCard(props: {
  label: string
  files: string
  diff: readonly [string, string]
  focused: boolean
}): JSX.Element {
  return (
    <div
      className={cn(
        'rounded-lg border px-3 py-2 transition-[background-color,border-color] duration-300',
        props.focused ? 'border-foreground/20 bg-accent' : 'border-border bg-background/70'
      )}
      data-candidate-focused={props.focused ? 'true' : 'false'}
    >
      <div className="flex items-center gap-2">
        <p className="truncate text-[11px] font-semibold">{props.label}</p>
        {props.focused ? (
          <span className="ml-auto text-[11px] text-muted-foreground">
            {copy('candidate.focused', 'Focused')}
          </span>
        ) : null}
      </div>
      <div className="mt-1 flex gap-2 font-mono text-[11px] text-muted-foreground">
        <span>{props.files}</span>
        <span className="[color:var(--git-decoration-added)]">{props.diff[0]}</span>
        <span className="[color:var(--git-decoration-deleted)]">{props.diff[1]}</span>
      </div>
    </div>
  )
}

function StoryPanel(props: {
  item: StoryItem
  index: StoryPhase
  reducedMotion: boolean
}): JSX.Element {
  const [Icon, kind, title, detail] = props.item
  return (
    <div
      className={cn(
        'flex min-h-[190px] flex-col p-4',
        !props.reducedMotion && 'animate-in fade-in-0 slide-in-from-bottom-1 duration-500'
      )}
      data-review-story-panel={kind}
    >
      <div className="flex items-start gap-3">
        <span className="flex size-8 shrink-0 items-center justify-center rounded-md border border-border bg-card">
          <Icon className="size-4 text-muted-foreground" />
        </span>
        <div>
          <p className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
            {props.index + 1}/{STORY.length} · {copy('timeline.title', 'Review path')}
          </p>
          <p className="mt-0.5 text-xs font-semibold">{copy(`story.${kind}.title`, title)}</p>
          <p className="mt-1 text-[11px] leading-relaxed text-muted-foreground">
            {copy(`story.${kind}.detail`, detail)}
          </p>
        </div>
      </div>
      <StoryEvidence kind={kind} />
    </div>
  )
}

function StoryEvidence(props: { kind: StoryKind }): JSX.Element {
  switch (props.kind) {
    case 'compare':
      return (
        <Evidence
          icon={GitCompareArrows}
          text={copy('evidence.compare', 'Open both diffs, then choose a candidate to refine.')}
        />
      )
    case 'annotate':
      return (
        <div className="mt-auto rounded-md border border-border bg-card p-2.5 text-[11px]">
          <p className="leading-relaxed text-muted-foreground">
            {copy(
              'evidence.note',
              'Review note: preserve restored scrollback before revealing the pane.'
            )}
          </p>
          <Action icon={Send} label={copy('evidence.send', 'Send revision')} primary />
        </div>
      )
    case 'decision':
      return (
        <Evidence
          icon={ShieldCheck}
          text={copy('evidence.decision', 'Human decision before checks or Git write actions.')}
        />
      )
    case 'blocked':
      return (
        <div className="mt-auto flex items-start gap-2 rounded-md border border-destructive/30 bg-destructive/10 p-2.5 text-[11px]">
          <CircleAlert className="mt-0.5 size-3.5 shrink-0 text-destructive" />
          <div>
            <p className="font-semibold">{copy('evidence.blocker', 'Example blocker')}</p>
            <p className="mt-0.5 text-muted-foreground">
              {copy(
                'evidence.blockerDetail',
                'A focused check fails or a merge conflict needs review.'
              )}
            </p>
          </div>
        </div>
      )
    case 'retry':
      return (
        <div className="mt-auto grid grid-cols-2 gap-2">
          <Action icon={GitMerge} label={copy('evidence.resolve', 'Resolve in workspace')} />
          <Action icon={RotateCcw} label={copy('evidence.retry', 'Retry focused checks')} />
        </div>
      )
    case 'passed':
      return (
        <div className="mt-auto flex items-center gap-2 rounded-md border border-status-success-border bg-status-success-background p-2.5 text-[11px] text-status-success">
          <CheckCircle2 className="size-3.5" />
          <span className="font-semibold">
            {copy('evidence.passed', 'Example result · checks pass')}
          </span>
        </div>
      )
    case 'rereview':
      return (
        <Evidence
          icon={ShieldCheck}
          text={copy(
            'evidence.rereview',
            'Human approval · recovered diff and refreshed checks reviewed'
          )}
        />
      )
    case 'stage':
      return (
        <Evidence
          icon={ListChecks}
          text={copy('evidence.stage', 'Stage focused hunk · src/terminal/session.ts')}
          focusedHunk
        />
      )
    case 'confirm':
      return (
        <div className="mt-auto flex items-center gap-2">
          <Action
            icon={GitCommitHorizontal}
            label={copy('evidence.commit', 'Confirm commit + push')}
          />
          <ArrowRight className="size-3 shrink-0 text-muted-foreground" />
          <Action
            icon={GitPullRequest}
            label={copy('evidence.review', 'Confirm PR / MR')}
            primary
          />
        </div>
      )
    case 'archive':
      return (
        <Evidence
          icon={Archive}
          text={copy('evidence.archive', 'Example end state · workspace archived.')}
        />
      )
  }
}

function Evidence(props: {
  icon: typeof Archive
  text: string
  focusedHunk?: boolean
}): JSX.Element {
  const Icon = props.icon
  return (
    <div
      className="mt-auto flex items-center gap-2 rounded-md border border-border bg-card p-2.5 text-[11px] text-muted-foreground"
      data-review-focused-hunk={props.focusedHunk ? 'staged' : undefined}
    >
      <Icon className="size-3.5 shrink-0" />
      <span>{props.text}</span>
    </div>
  )
}

function Action(props: { icon: typeof Archive; label: string; primary?: boolean }): JSX.Element {
  const Icon = props.icon
  return (
    <span
      className={cn(
        'mt-2 inline-flex min-w-0 flex-1 items-center gap-1.5 rounded-md border px-2 py-1.5 text-[11px] font-medium',
        props.primary
          ? 'border-primary bg-primary text-primary-foreground'
          : 'border-border bg-card'
      )}
    >
      <Icon className="size-3 shrink-0" />
      <span className="leading-tight">{props.label}</span>
    </span>
  )
}

function TimelineStep(props: {
  item: StoryItem
  index: number
  phase: StoryPhase
  last: boolean
}): JSX.Element {
  const [Icon, kind, title] = props.item
  const complete = props.index < props.phase
  const active = props.index === props.phase
  return (
    <div
      className="relative flex min-h-9 gap-2.5 pb-2 last:pb-0"
      data-state={active ? 'active' : complete ? 'complete' : 'upcoming'}
    >
      {!props.last ? <span className="absolute bottom-0 left-[11px] top-6 w-px bg-border" /> : null}
      <span
        className={cn(
          'relative z-10 flex size-6 shrink-0 items-center justify-center rounded-full border bg-card text-muted-foreground transition-colors duration-300',
          active && 'border-foreground/20 bg-accent text-accent-foreground'
        )}
      >
        {complete ? <Check className="size-3" /> : <Icon className="size-3" />}
      </span>
      <p
        className={cn(
          'pt-1 text-[11px] leading-tight text-muted-foreground',
          active && 'font-semibold text-foreground'
        )}
      >
        {copy(`story.${kind}.title`, title)}
      </p>
    </div>
  )
}
