import type { AgentType, ParsedAgentStatusPayload } from '../../../../shared/agent-status-types'
import type { GlobalSettings } from '../../../../shared/types'
import type { RecognizedAgentProcess } from '../../../../shared/agent-process-recognition'
import type { RuntimeTerminalProcessInspection } from '@/runtime/runtime-terminal-inspection'

export type AgentCompletionStatusSnapshot = ParsedAgentStatusPayload & {
  stateStartedAt?: number
  /** Raw agent hook event name (e.g. UserPromptSubmit, PreToolUse, Stop), when
   *  the hook IPC path forwards it. Absent on the OSC/title and remote-runtime
   *  paths, which carry no hook event identity. */
  hookEventName?: string
  /** True when the originating hook event carried prompt text directly — the
   *  new-turn boundary signal. Absent unless the hook IPC path forwarded it. */
  hasExplicitPrompt?: boolean
}

export type AgentCompletionDispatchMeta = {
  source: 'hook' | 'title' | 'process-exit'
  quietedHookDone: boolean
  terminalIdleConfirmed?: boolean
  agentStatus?: AgentCompletionStatusSnapshot
}

export type AgentAttentionDispatchMeta = {
  source: 'hook'
  agentStatus: AgentCompletionStatusSnapshot
}

export type AgentCompletionStatusRepairSignal =
  | {
      source: 'title'
      title: string
      agentType?: AgentType
    }
  | {
      source: 'process-exit'
      title: string
      agent: RecognizedAgentProcess
    }

export type AgentCompletionCoordinatorOptions = {
  paneKey: string
  getPtyId: () => string | null
  getSettings: () => Pick<GlobalSettings, 'activeRuntimeEnvironmentId'> | null | undefined
  inspectProcess: (
    settings: Pick<GlobalSettings, 'activeRuntimeEnvironmentId'> | null | undefined,
    ptyId: string
  ) => Promise<RuntimeTerminalProcessInspection>
  dispatchCompletion: (title: string, meta?: AgentCompletionDispatchMeta) => void
  dispatchAttention?: (title: string, meta: AgentAttentionDispatchMeta) => void
  dispatchHookLifecycle?: (payload: AgentCompletionStatusSnapshot) => void
  shouldSuppressProcessReplacementCompletion?: (
    exited: RecognizedAgentProcess,
    replacement: RecognizedAgentProcess
  ) => boolean
  shouldSuppressConfirmedProcessExitCompletion?: (exited: RecognizedAgentProcess) => boolean
  isLive: () => boolean
  shouldPollProcessCadence?: () => boolean
  // Why: on hosts where one inspection forks a whole-process-table scan (local
  // Windows PowerShell/CIM), panes without agent evidence relax to a slow
  // cadence; cheap hosts (POSIX `ps`, SSH/remote-owned scans) keep full cadence.
  isProcessInspectionCostly?: () => boolean
  shouldSuppressHookCompletion?: (payload: AgentCompletionStatusSnapshot) => boolean
  // Why: title/process completion can prove a turn ended when the agent missed
  // its final hook (#7202); the pane repairs the stuck 'working' status row and
  // returns the synthesized snapshot for the completion notification.
  onCompletionStatusRepair?: (
    signal: AgentCompletionStatusRepairSignal
  ) => AgentCompletionStatusSnapshot | null | undefined
}

export type AgentCompletionCoordinator = {
  observeTitle: (title: string) => void
  observeClassifiedTitleCompletion: (title: string) => void
  observeTitleWorking: () => void
  observeOutputActivity: () => void
  observeHookStatus: (payload: AgentCompletionStatusSnapshot) => void
  startProcessTracking: () => void
  hasPendingHookDoneCompletion: () => boolean
  resetCompletionState: (options?: { requireFreshWorking?: boolean }) => void
  dispose: () => void
}
