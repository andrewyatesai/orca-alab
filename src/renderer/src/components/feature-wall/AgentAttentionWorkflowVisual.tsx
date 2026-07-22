import { useEffect, useState } from 'react'
import type { JSX, ReactNode } from 'react'
import {
  BellRing,
  CheckCircle2,
  ChevronDown,
  History,
  Inbox,
  MessageSquareReply,
  TerminalSquare
} from 'lucide-react'
import { AgentStateDot, type AgentDotState } from '@/components/AgentStateDot'
import { AgentIcon } from '@/lib/agent-catalog'
import { translate } from '@/i18n/i18n'
import { cn } from '@/lib/utils'
import { AiVaultSessionHistoryStrip } from './AiVaultSessionHistoryStrip'
import { AgentAutonomyBoundary } from './AgentAutonomyBoundary'

type StoryPhase = 0 | 1 | 2 | 3 | 4 | 5

const PHASE_NAMES = ['fleet', 'attention', 'replied', 'usage', 'switched', 'resumed'] as const
const PHASE_DURATIONS_MS: readonly number[] = [600, 700, 600, 500, 500]

function getPhaseDuration(phase: StoryPhase): number {
  return PHASE_DURATIONS_MS[phase] ?? 0
}

export function AgentAttentionWorkflowVisual(props: { reducedMotion: boolean }): JSX.Element {
  const [animatedPhase, setAnimatedPhase] = useState<StoryPhase>(0)

  useEffect(() => {
    if (props.reducedMotion) {
      return
    }
    let phase: StoryPhase = 0
    let timeoutId: number | undefined
    const advance = (): void => {
      if (phase >= PHASE_NAMES.length - 1) {
        return
      }
      phase = (phase + 1) as StoryPhase
      setAnimatedPhase(phase)
      if (phase < PHASE_NAMES.length - 1) {
        timeoutId = window.setTimeout(advance, getPhaseDuration(phase))
      }
    }
    timeoutId = window.setTimeout(advance, getPhaseDuration(phase))
    return () => {
      if (timeoutId !== undefined) {
        window.clearTimeout(timeoutId)
      }
    }
  }, [props.reducedMotion])

  // Why: the static accessibility preference should show the complete story,
  // not the animation's empty opening beat.
  const phase: StoryPhase = props.reducedMotion ? 5 : animatedPhase
  const attentionOpen = phase === 1
  const replied = phase >= 2
  const resumed = phase >= 5

  return (
    <div
      className="w-full max-w-[680px] overflow-hidden rounded-xl border border-border bg-card text-card-foreground shadow-xs"
      data-feature-wall-agent-attention-visual
      data-story-phase={PHASE_NAMES[phase]}
      aria-hidden
    >
      <header className="flex h-10 items-center gap-3 border-b border-border px-3.5">
        <div className="flex min-w-0 items-center gap-2.5">
          <div className="flex size-7 shrink-0 items-center justify-center rounded-md border border-border bg-muted/40">
            <TerminalSquare className="size-3.5 text-muted-foreground" />
          </div>
          <div className="min-w-0">
            <p className="truncate text-xs font-semibold">
              {translate('auto.fw.agentAttention.f130000001', 'Agent fleet')}
            </p>
            <p className="truncate text-[11px] text-muted-foreground">
              {translate(
                'auto.fw.agentAttention.f130000002',
                'Supported and custom terminal agents'
              )}
            </p>
          </div>
        </div>
        <div
          className={cn(
            'ml-auto flex items-center gap-1.5 rounded-full border border-border bg-muted/30 px-2.5 py-1 text-[11px] font-medium text-muted-foreground transition-opacity duration-300',
            resumed ? 'opacity-100' : 'opacity-0'
          )}
        >
          <History className="size-3" />
          {translate('auto.fw.agentAttention.f130000003', 'Reconnected · 3 sessions restored')}
        </div>
      </header>

      <div className="grid sm:grid-cols-[minmax(0,1fr)_240px]">
        <section className="border-b border-border p-3 sm:border-b-0 sm:border-r">
          <SectionLabel
            label={translate('auto.fw.agentAttention.f130000004', 'Active sessions')}
            meta={translate('auto.fw.agentAttention.f130000005', '3 agents')}
          />
          <div className="mt-2 space-y-1.5">
            <AgentRow
              icon={<AgentIcon agent="claude" size={16} />}
              name={translate('auto.fw.agentAttention.f130000006', 'Claude')}
              kind={translate('auto.fw.agentAttention.f130000007', 'Supported')}
              state="working"
              status={translate('auto.fw.agentAttention.f130000044', 'Working')}
              detail={translate('auto.fw.agentAttention.f130000008', 'Refining the reconnect flow')}
            />
            <AgentRow
              icon={<AgentIcon agent="codex" size={16} />}
              name={translate('auto.fw.agentAttention.f130000009', 'Codex')}
              kind={translate('auto.fw.agentAttention.f130000007', 'Supported')}
              state={attentionOpen ? 'permission' : 'working'}
              status={
                attentionOpen
                  ? translate('auto.fw.agentAttention.f130000024', 'Needs attention')
                  : translate('auto.fw.agentAttention.f130000044', 'Working')
              }
              detail={
                attentionOpen
                  ? translate(
                      'auto.fw.agentAttention.f130000010',
                      'Needs approval to run focused tests'
                    )
                  : replied
                    ? translate(
                        'auto.fw.agentAttention.f130000011',
                        'Running the approved test suite'
                      )
                    : translate('auto.fw.agentAttention.f130000012', 'Tracing session state')
              }
              emphasized={attentionOpen}
            />
            <AgentRow
              icon={<TerminalSquare className="size-4" />}
              name={translate('auto.fw.agentAttention.f130000013', 'Release QA')}
              kind={translate('auto.fw.agentAttention.f130000014', 'Custom CLI')}
              state="done"
              status={translate('auto.fw.agentAttention.f130000045', 'Done')}
              detail={translate(
                'auto.fw.agentAttention.f130000015',
                'Checked Windows and SSH behavior'
              )}
            />
          </div>

          <div className="mt-2 min-h-[76px] rounded-lg border border-border bg-muted/20 p-2.5">
            <div className="flex items-center gap-2 text-[11px] font-semibold">
              <MessageSquareReply className="size-3.5 text-muted-foreground" />
              {translate('auto.fw.agentAttention.f130000016', 'Session response')}
              {resumed ? (
                <span className="ml-auto text-[11px] font-normal text-muted-foreground">
                  {translate(
                    'auto.fw.agentAttention.f130000017',
                    'Transcript + scrollback restored'
                  )}
                </span>
              ) : null}
            </div>
            <div className="mt-2 grid gap-1.5 text-[11px]">
              <MessageLine
                label={translate('auto.fw.agentAttention.f130000018', 'You')}
                text={
                  replied
                    ? translate('auto.fw.agentAttention.f130000019', 'Run the focused test suite.')
                    : translate(
                        'auto.fw.agentAttention.f130000020',
                        'Waiting for an agent request…'
                      )
                }
                visible={replied}
              />
              <MessageLine
                label={translate('auto.fw.agentAttention.f130000009', 'Codex')}
                text={translate(
                  'auto.fw.agentAttention.f130000021',
                  'Continuing in the same terminal session.'
                )}
                visible={replied}
              />
            </div>
          </div>
        </section>

        <aside className="space-y-2 bg-muted/10 p-3">
          <InboxCard attentionOpen={attentionOpen} replied={replied} />
          <UsageCard focused={phase >= 3} switched={phase >= 4} />
        </aside>
      </div>

      <AiVaultSessionHistoryStrip />
      <AgentAutonomyBoundary />
    </div>
  )
}

function SectionLabel(props: { label: string; meta: string }): JSX.Element {
  return (
    <div className="flex items-center justify-between text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
      <span>{props.label}</span>
      <span>{props.meta}</span>
    </div>
  )
}

function AgentRow(props: {
  icon: JSX.Element
  name: string
  kind: string
  state: AgentDotState
  status: string
  detail: string
  emphasized?: boolean
}): JSX.Element {
  return (
    <div
      className={cn(
        'grid grid-cols-[16px_minmax(0,1fr)_auto] items-center gap-2 rounded-lg border px-2.5 py-1.5 transition-colors duration-300',
        props.emphasized ? 'border-foreground/20 bg-accent' : 'border-transparent bg-muted/20'
      )}
    >
      <span className="flex size-4 items-center justify-center">{props.icon}</span>
      <div className="min-w-0">
        <div className="flex items-center gap-1.5">
          <span className="truncate text-[11px] font-semibold">{props.name}</span>
          <span className="rounded-full border border-border bg-background px-1.5 py-px text-[11px] text-muted-foreground">
            {props.kind}
          </span>
        </div>
        <p className="mt-0.5 truncate text-[11px] text-muted-foreground">{props.detail}</p>
      </div>
      <span className="flex items-center gap-1 text-[11px] font-medium text-muted-foreground">
        <AgentStateDot state={props.state} size="md" />
        {props.status}
      </span>
    </div>
  )
}

function MessageLine(props: { label: string; text: string; visible: boolean }): JSX.Element {
  return (
    <div
      className={cn(
        'flex gap-2 transition-opacity duration-300',
        props.visible ? '' : 'opacity-40'
      )}
    >
      <span className="w-10 shrink-0 font-medium text-muted-foreground">{props.label}</span>
      <span className="min-w-0 truncate">{props.text}</span>
    </div>
  )
}

function InboxCard(props: { attentionOpen: boolean; replied: boolean }): JSX.Element {
  return (
    <div
      className={cn(
        'rounded-lg border p-2.5 transition-[background-color,border-color] duration-300',
        props.attentionOpen ? 'border-foreground/20 bg-accent' : 'border-border bg-card'
      )}
    >
      <SectionLabel
        label={translate('auto.fw.agentAttention.f130000022', 'Inbox')}
        meta={props.attentionOpen ? '1' : '0'}
      />
      <div className="mt-2 flex items-start gap-2">
        <div className="flex size-7 shrink-0 items-center justify-center rounded-md border border-border bg-background">
          {props.replied ? (
            <CheckCircle2 className="size-3.5 text-status-success" />
          ) : props.attentionOpen ? (
            <BellRing className="size-3.5" />
          ) : (
            <Inbox className="size-3.5 text-muted-foreground" />
          )}
        </div>
        <div className="min-w-0 flex-1">
          <p className="text-[11px] font-semibold">
            {props.replied
              ? translate('auto.fw.agentAttention.f130000023', 'Needs attention · resolved')
              : props.attentionOpen
                ? translate('auto.fw.agentAttention.f130000024', 'Needs attention')
                : translate('auto.fw.agentAttention.f130000025', 'Watching all sessions')}
          </p>
          <p className="mt-1 text-[11px] leading-snug text-muted-foreground">
            {props.replied
              ? translate(
                  'auto.fw.agentAttention.f130000026',
                  'Opened from Inbox · reply sent to Codex'
                )
              : translate(
                  'auto.fw.agentAttention.f130000027',
                  'Jump directly to waiting or blocked work.'
                )}
          </p>
        </div>
      </div>
      {props.attentionOpen ? (
        <div className="mt-2 rounded-md bg-foreground px-2 py-1 text-center text-[11px] font-medium text-background">
          {translate('auto.fw.agentAttention.f130000028', 'Open in Inbox')}
        </div>
      ) : null}
    </div>
  )
}

function UsageCard(props: { focused: boolean; switched: boolean }): JSX.Element {
  const codexUsed = props.switched ? 18 : 94
  return (
    <div
      className={cn(
        'rounded-lg border border-border bg-card p-2.5 transition-opacity duration-300',
        props.focused ? 'opacity-100' : 'opacity-60'
      )}
    >
      <SectionLabel
        label={translate('auto.fw.agentAttention.f130000029', 'Usage & accounts')}
        meta={translate('auto.fw.agentAttention.f130000030', 'Example state')}
      />
      <div className="mt-2 flex items-center justify-between text-[11px]">
        <span className="font-medium">
          {translate('auto.fw.agentAttention.f130000031', 'Codex account')}
        </span>
        <span className="inline-flex items-center gap-1 rounded-md bg-muted/60 px-1.5 py-1 text-muted-foreground">
          {props.switched
            ? translate('auto.fw.agentAttention.f130000032', 'Team')
            : translate('auto.fw.agentAttention.f130000033', 'Personal')}
          <ChevronDown
            className={cn(
              'size-2.5 transition-transform duration-300',
              props.focused && !props.switched ? 'rotate-180' : ''
            )}
          />
        </span>
      </div>
      <UsageMeter
        value={codexUsed}
        danger={!props.switched}
        meta={translate('auto.fw.agentAttention.f130000034', '{{value0}}% used · resets in 5h', {
          value0: codexUsed
        })}
      />
      <div className="mt-2">
        <div className="flex justify-between text-[11px]">
          <span className="font-medium">
            {translate('auto.fw.agentAttention.f130000035', 'Claude weekly')}
          </span>
          <span className="text-muted-foreground">
            {translate('auto.fw.agentAttention.f130000036', '41% used')}
          </span>
        </div>
        <UsageMeter value={41} meta={null} />
      </div>
      <p
        className={cn(
          'mt-2 text-[11px] text-muted-foreground transition-opacity duration-300',
          props.switched ? 'opacity-100' : 'opacity-0'
        )}
      >
        {props.switched
          ? translate('auto.fw.agentAttention.f130000037', 'Switched from Personal at 94% used')
          : translate('auto.fw.agentAttention.f130000046', 'Switch to Team · 18% used')}
      </p>
    </div>
  )
}

function UsageMeter(props: { value: number; danger?: boolean; meta: ReactNode }): JSX.Element {
  return (
    <div className="mt-1">
      <div className="h-1.5 overflow-hidden rounded-full bg-muted">
        <span
          className={cn(
            'block h-full rounded-full transition-[width,background-color] duration-700',
            props.danger ? 'bg-destructive' : 'bg-status-success'
          )}
          style={{ width: `${props.value}%` }}
        />
      </div>
      {props.meta ? (
        <p className="mt-1 text-right font-mono text-[11px] text-muted-foreground">{props.meta}</p>
      ) : null}
    </div>
  )
}
