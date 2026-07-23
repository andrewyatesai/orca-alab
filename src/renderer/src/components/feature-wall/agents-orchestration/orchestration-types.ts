// Why: the walkthrough must show the full dependency/recovery story once and settle before five seconds.
export const BUBBLE_FLIGHT_MS = 260
export const BUBBLE_LAND_MS = BUBBLE_FLIGHT_MS + 260
export const BUBBLE_GAP_MS = 420
export const RESPONSE_BEAT_GAP_MS = 380
export const ORCHESTRATION_CLI_COMMAND_TIMINGS_MS = [120, 480, 800, 1160] as const

export type AgentKey = 'coord-claude' | 'child-codex' | 'child-claude'

export type OrchestrationMessageId =
  | 'coord-planning'
  | 'coord-dispatching-migration'
  | 'coord-linking-dependency'
  | 'coord-decision-gate'
  | 'coord-human-decision-resolved'
  | 'coord-decision-recorded'
  | 'coord-releasing-dependency'
  | 'coord-recovery-gate'
  | 'coord-recovery-plan'
  | 'coord-result-complete'
  | 'migration-waiting-dispatch'
  | 'migration-running'
  | 'migration-blocking-question'
  | 'migration-applying-decision'
  | 'migration-complete'
  | 'middleware-waiting-dependency'
  | 'middleware-dependency-released'
  | 'middleware-check-blocked'
  | 'middleware-rerunning-check'
  | 'middleware-recovered'

export type OrchestrationPhase =
  | 'plan'
  | 'dispatch'
  | 'dependency'
  | 'question'
  | 'decision'
  | 'relay'
  | 'unblocked'
  | 'blocker'
  | 'recovery'
  | 'complete'

export const ORCHESTRATION_PHASE_SEQUENCE: readonly OrchestrationPhase[] = [
  'plan',
  'dispatch',
  'dependency',
  'question',
  'decision',
  'relay',
  'unblocked',
  'blocker',
  'recovery',
  'complete'
]

export type AgentRowState = 'working' | 'waiting' | 'question' | 'blocked' | 'done'

export type Beat = {
  actor?: 'human' | 'coordinator'
  delivery?: 'bubble' | 'local'
  from: AgentKey
  to: AgentKey
  phase: OrchestrationPhase
  senderMessage?: OrchestrationMessageId
  recipientMessage?: OrchestrationMessageId
  senderState?: AgentRowState
  recipientState?: AgentRowState
}

export const ORCHESTRATION_BEATS: readonly Beat[] = [
  {
    from: 'coord-claude',
    to: 'child-codex',
    phase: 'dispatch',
    senderMessage: 'coord-dispatching-migration',
    recipientMessage: 'migration-running',
    recipientState: 'working'
  },
  {
    from: 'coord-claude',
    to: 'child-claude',
    phase: 'dependency',
    senderMessage: 'coord-linking-dependency',
    recipientMessage: 'middleware-waiting-dependency',
    recipientState: 'waiting'
  },
  {
    from: 'child-codex',
    to: 'coord-claude',
    phase: 'question',
    senderMessage: 'migration-blocking-question',
    recipientMessage: 'coord-decision-gate',
    senderState: 'question',
    recipientState: 'waiting'
  },
  {
    actor: 'human',
    delivery: 'local',
    from: 'coord-claude',
    to: 'coord-claude',
    phase: 'decision',
    senderMessage: 'coord-human-decision-resolved',
    senderState: 'working'
  },
  {
    actor: 'coordinator',
    from: 'coord-claude',
    to: 'child-codex',
    phase: 'relay',
    senderMessage: 'coord-decision-recorded',
    recipientMessage: 'migration-applying-decision',
    senderState: 'working',
    recipientState: 'working'
  },
  {
    from: 'child-codex',
    to: 'coord-claude',
    phase: 'unblocked',
    senderMessage: 'migration-complete',
    recipientMessage: 'coord-releasing-dependency',
    senderState: 'done',
    recipientState: 'working'
  },
  {
    from: 'coord-claude',
    to: 'child-claude',
    phase: 'unblocked',
    senderMessage: 'coord-releasing-dependency',
    recipientMessage: 'middleware-dependency-released',
    recipientState: 'working'
  },
  {
    from: 'child-claude',
    to: 'coord-claude',
    phase: 'blocker',
    senderMessage: 'middleware-check-blocked',
    recipientMessage: 'coord-recovery-gate',
    senderState: 'blocked',
    recipientState: 'waiting'
  },
  {
    from: 'coord-claude',
    to: 'child-claude',
    phase: 'recovery',
    senderMessage: 'coord-recovery-plan',
    recipientMessage: 'middleware-rerunning-check',
    senderState: 'working',
    recipientState: 'working'
  },
  {
    from: 'child-claude',
    to: 'coord-claude',
    phase: 'complete',
    senderMessage: 'middleware-recovered',
    recipientMessage: 'coord-result-complete',
    senderState: 'done',
    recipientState: 'done'
  }
]

export type RowState = Record<AgentKey, AgentRowState>
export type RowMessages = Record<AgentKey, OrchestrationMessageId>
export type RowFlash = Partial<Record<AgentKey, number>>
export type RowPending = Partial<Record<AgentKey, boolean>>

export const INITIAL_ROW_STATE: RowState = {
  'coord-claude': 'working',
  'child-codex': 'waiting',
  'child-claude': 'waiting'
}

export const INITIAL_ROW_MESSAGES: RowMessages = {
  'coord-claude': 'coord-planning',
  'child-codex': 'migration-waiting-dispatch',
  'child-claude': 'middleware-waiting-dependency'
}

export const COMPLETED_ROW_STATE: RowState = {
  'coord-claude': 'done',
  'child-codex': 'done',
  'child-claude': 'done'
}

export const COMPLETED_ROW_MESSAGES: RowMessages = {
  'coord-claude': 'coord-result-complete',
  'child-codex': 'migration-complete',
  'child-claude': 'middleware-recovered'
}
