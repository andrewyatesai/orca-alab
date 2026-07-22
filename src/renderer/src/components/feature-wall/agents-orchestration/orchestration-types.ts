// Beat timings — slow enough to read at a glance.
export const BUBBLE_FLIGHT_MS = 1600
export const BUBBLE_LAND_MS = BUBBLE_FLIGHT_MS + 360
export const BUBBLE_GAP_MS = 3400
export const ORCHESTRATION_CLI_COMMAND_TIMINGS_MS = [250, 2500, 5200, 8600] as const
export const ORCHESTRATION_CLI_COMMAND_LOOP_MS = 12800

export type AgentKey = 'coord-claude' | 'child-codex' | 'child-claude'

export type OrchestrationMessageId =
  | 'coord-splitting'
  | 'child-codex-writing'
  | 'child-claude-sketching'
  | 'child-codex-adding-column'
  | 'child-claude-wiring-middleware'
  | 'child-codex-ready'
  | 'child-claude-ready'

export type Beat = {
  from: AgentKey
  to: AgentKey
  recipientMsg?: OrchestrationMessageId
  coordMsg?: OrchestrationMessageId
  // The "send" *is* the "finish" — flipping the spinner to a check the
  // moment the bubble departs reads as the agent wrapping up and reporting
  // back to the orchestrator.
  senderFinishes?: boolean
}

export const PHASE1_BEATS: readonly Beat[] = [
  {
    from: 'coord-claude',
    to: 'child-codex',
    recipientMsg: 'child-codex-adding-column'
  },
  {
    from: 'coord-claude',
    to: 'child-claude',
    recipientMsg: 'child-claude-wiring-middleware'
  },
  {
    from: 'child-codex',
    to: 'coord-claude',
    coordMsg: 'child-codex-ready',
    senderFinishes: true
  },
  {
    from: 'child-claude',
    to: 'coord-claude',
    coordMsg: 'child-claude-ready',
    senderFinishes: true
  }
]

export const COORD_INITIAL_MSG: OrchestrationMessageId = 'coord-splitting'
export const CHILD_CODEX_INITIAL_MSG: OrchestrationMessageId = 'child-codex-writing'
export const CHILD_CLAUDE_INITIAL_MSG: OrchestrationMessageId = 'child-claude-sketching'

export type AgentRowState = 'working' | 'done'

export type RowState = Record<AgentKey, AgentRowState>
export type RowMessages = Record<AgentKey, OrchestrationMessageId>
export type RowFlash = Partial<Record<AgentKey, number>>
export type RowPending = Partial<Record<AgentKey, boolean>>

export const INITIAL_ROW_STATE: RowState = {
  'coord-claude': 'working',
  'child-codex': 'working',
  'child-claude': 'working'
}

export const INITIAL_ROW_MESSAGES: RowMessages = {
  'coord-claude': COORD_INITIAL_MSG,
  'child-codex': CHILD_CODEX_INITIAL_MSG,
  'child-claude': CHILD_CLAUDE_INITIAL_MSG
}
