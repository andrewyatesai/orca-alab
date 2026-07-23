import type { JSX } from 'react'
import { CircleAlert, CircleCheck, CircleHelp, GitBranch, RotateCcw, Workflow } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { translate } from '@/i18n/i18n'
import { ClaudeIcon, OpenAIIcon } from '../../status-bar/icons'
import { AgentRow, WorkspaceCard } from './orchestration-cards'
import { getOrchestrationLedgerCopy } from './orchestration-ledger-copy'
import {
  getOrchestrationMessage,
  getOrchestrationPhaseCopy,
  getOrchestrationWorkspaceName
} from './orchestration-storyboard-copy'
import type {
  AgentKey,
  OrchestrationPhase,
  RowFlash,
  RowMessages,
  RowPending,
  RowState
} from './orchestration-types'

export function OrchestrationWorkspaceCards(props: {
  displayedChildCount: number
  phase: OrchestrationPhase
  registerRow: (agent: AgentKey, node: HTMLDivElement | null) => void
  rowFlash: RowFlash
  rowMessages: RowMessages
  rowPending: RowPending
  rowState: RowState
  showRunStatus: boolean
  showSettledReducedState: boolean
}): JSX.Element {
  return (
    <div className="relative flex min-w-0 flex-col gap-2.5">
      {props.showRunStatus ? <OrchestrationRunStatus phase={props.phase} /> : null}
      <WorkspaceCard
        variant="coordinator"
        name={getOrchestrationWorkspaceName('coordinator')}
        dataCard="coord"
        rows={[
          <AgentRow
            key="coord-claude"
            agentKey="coord-claude"
            icon={<ClaudeIcon size={13} />}
            state={props.rowState['coord-claude']}
            message={getOrchestrationMessage(props.rowMessages['coord-claude'])}
            flashKey={props.rowFlash['coord-claude'] ?? 0}
            registerRef={(node) => props.registerRow('coord-claude', node)}
          />
        ]}
      />

      <div
        className="flex justify-start"
        style={{ marginLeft: 'var(--feature-wall-child-indent, 28px)' }}
      >
        <span
          className="inline-flex h-5 items-center gap-1 rounded-md border border-border bg-card px-1.5 text-[11px] font-medium text-muted-foreground"
          aria-label={translate(
            'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000063',
            '2-worker dependency graph'
          )}
        >
          <Workflow className="size-2.5" aria-hidden />
          <span className="truncate">
            {translate(
              'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000064',
              '2 workers · Task 2 depends on Task 1'
            )}
          </span>
        </span>
      </div>

      <div
        className="feature-wall-children-wrapper ml-auto flex flex-col gap-2"
        data-visible={props.displayedChildCount > 0 ? 'true' : undefined}
        style={{
          width: 'calc(100% - var(--feature-wall-child-indent, 28px))',
          ...(props.showSettledReducedState
            ? { opacity: 1, transform: 'none', transition: 'none' }
            : {})
        }}
      >
        {props.displayedChildCount >= 1 ? (
          <ChildCardShell settled={props.showSettledReducedState}>
            <WorkspaceCard
              variant="default"
              name={getOrchestrationWorkspaceName('migration')}
              dataCard="child"
              childPadding
              rows={[
                <AgentRow
                  key="child-codex"
                  agentKey="child-codex"
                  icon={<OpenAIIcon size={13} />}
                  state={props.rowState['child-codex']}
                  message={getOrchestrationMessage(props.rowMessages['child-codex'])}
                  flashKey={props.rowFlash['child-codex'] ?? 0}
                  pending={props.showSettledReducedState ? false : props.rowPending['child-codex']}
                  spawnRow
                  registerRef={(node) => props.registerRow('child-codex', node)}
                />
              ]}
            />
          </ChildCardShell>
        ) : null}
        {props.displayedChildCount >= 2 ? (
          <ChildCardShell settled={props.showSettledReducedState}>
            <WorkspaceCard
              variant="default"
              name={getOrchestrationWorkspaceName('middleware')}
              dataCard="child-claude"
              childPadding
              rows={[
                <AgentRow
                  key="child-claude"
                  agentKey="child-claude"
                  icon={<ClaudeIcon size={13} />}
                  state={props.rowState['child-claude']}
                  message={getOrchestrationMessage(props.rowMessages['child-claude'])}
                  flashKey={props.rowFlash['child-claude'] ?? 0}
                  pending={props.showSettledReducedState ? false : props.rowPending['child-claude']}
                  spawnRow
                  registerRef={(node) => props.registerRow('child-claude', node)}
                />
              ]}
            />
          </ChildCardShell>
        ) : null}
      </div>
    </div>
  )
}

function ChildCardShell(props: { settled: boolean; children: JSX.Element }): JSX.Element {
  return (
    <div
      className="feature-wall-child-card-shell"
      style={props.settled ? { animation: 'none', opacity: 1, transform: 'none' } : undefined}
    >
      {props.children}
    </div>
  )
}

function OrchestrationRunStatus(props: { phase: OrchestrationPhase }): JSX.Element {
  const phaseCopy = getOrchestrationPhaseCopy(props.phase)
  const ledger = getOrchestrationLedgerCopy(props.phase)
  return (
    <div
      className="rounded-lg border border-border bg-muted/20 px-3 py-2 shadow-xs"
      data-feature-wall-orchestration-phase={props.phase}
    >
      <div className="flex min-w-0 items-center gap-2">
        <Badge
          variant={props.phase === 'blocker' ? 'destructive' : 'outline'}
          className="h-5 max-w-[140px] px-2 text-[11px]"
        >
          <PhaseIcon phase={props.phase} />
          <span className="truncate">{phaseCopy.label}</span>
        </Badge>
        <p className="truncate text-xs font-semibold text-foreground">{phaseCopy.headline}</p>
      </div>
      <div className="mt-2 grid grid-cols-3 divide-x divide-border border-t border-border pt-2">
        <LedgerCell
          kind="dependency"
          label={translate(
            'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000050',
            'Dependency'
          )}
          value={ledger.dependency}
          state={dependencyState(props.phase)}
        />
        <LedgerCell
          kind="decision"
          label={translate(
            'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000051',
            'Decision'
          )}
          value={ledger.decision}
          state={decisionState(props.phase)}
        />
        <LedgerCell
          kind="recovery"
          label={translate(
            'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000052',
            'Recovery'
          )}
          value={ledger.recovery}
          state={recoveryState(props.phase)}
        />
      </div>
    </div>
  )
}

function LedgerCell(props: {
  kind: 'dependency' | 'decision' | 'recovery'
  label: string
  value: string
  state: string
}): JSX.Element {
  return (
    <div
      className="min-w-0 px-2 first:pl-0 last:pr-0"
      data-orchestration-ledger={props.kind}
      data-state={props.state}
    >
      <p className="text-[11px] font-semibold uppercase text-muted-foreground">{props.label}</p>
      {/* Why: decision evidence must stay legible when localized instead of disappearing in an ellipsis. */}
      <p className="line-clamp-2 text-[11px] leading-tight text-foreground">{props.value}</p>
    </div>
  )
}

function PhaseIcon(props: { phase: OrchestrationPhase }): JSX.Element {
  if (props.phase === 'question' || props.phase === 'decision' || props.phase === 'relay') {
    return <CircleHelp aria-hidden />
  }
  if (props.phase === 'blocker') {
    return <CircleAlert aria-hidden />
  }
  if (props.phase === 'recovery') {
    return <RotateCcw aria-hidden />
  }
  if (props.phase === 'complete') {
    return <CircleCheck aria-hidden />
  }
  return <GitBranch aria-hidden />
}

function dependencyState(phase: OrchestrationPhase): 'waiting' | 'released' {
  return ['unblocked', 'blocker', 'recovery', 'complete'].includes(phase) ? 'released' : 'waiting'
}

function decisionState(phase: OrchestrationPhase): 'pending' | 'required' | 'resolved' {
  if (phase === 'question') {
    return 'required'
  }
  return ['decision', 'relay', 'unblocked', 'blocker', 'recovery', 'complete'].includes(phase)
    ? 'resolved'
    : 'pending'
}

function recoveryState(phase: OrchestrationPhase): 'watching' | 'blocked' | 'active' | 'recovered' {
  if (phase === 'blocker') {
    return 'blocked'
  }
  if (phase === 'recovery') {
    return 'active'
  }
  return phase === 'complete' ? 'recovered' : 'watching'
}
