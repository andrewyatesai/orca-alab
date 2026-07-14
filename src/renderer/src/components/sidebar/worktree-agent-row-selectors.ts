import type { RetainedAgentEntry } from '@/store/slices/agent-status'
import type { AppState } from '@/store/types'
import type {
  AgentStatusEntry,
  AgentStatusOrchestrationContext,
  MigrationUnsupportedPtyEntry
} from '../../../../shared/agent-status-types'
import type { TerminalLayoutSnapshot } from '../../../../shared/types'
import {
  getLiveEntriesByWorktree,
  getMigrationUnsupportedByWorktree,
  getRetainedEntriesByWorktree,
  getRuntimeAgentOrchestrationByWorktree,
  type RuntimeAgentOrchestrationState,
  type WorktreeAgentRowsState
} from './worktree-agent-index-cache'

const EMPTY_LIVE_ENTRIES: AgentStatusEntry[] = []
const EMPTY_MIGRATION_UNSUPPORTED_ENTRIES: MigrationUnsupportedPtyEntry[] = []
const EMPTY_RETAINED: RetainedAgentEntry[] = []
const EMPTY_RUNTIME_AGENT_ORCHESTRATION: Record<string, AgentStatusOrchestrationContext> = {}
// Why: selector unit tests often pass partial store mocks; production state
// owns these maps, but missing mock maps should behave like empty slices.
const EMPTY_RECORD = {}

export function selectLiveAgentStatusEntriesForWorktree(
  state: WorktreeAgentRowsState,
  worktreeId: string
): AgentStatusEntry[] {
  return getLiveEntriesByWorktree(state).get(worktreeId) ?? EMPTY_LIVE_ENTRIES
}

export function selectMigrationUnsupportedEntriesForWorktree(
  state: WorktreeAgentRowsState,
  worktreeId: string
): MigrationUnsupportedPtyEntry[] {
  return (
    getMigrationUnsupportedByWorktree(state).get(worktreeId) ?? EMPTY_MIGRATION_UNSUPPORTED_ENTRIES
  )
}

export function selectRetainedAgentEntriesForWorktree(
  state: WorktreeAgentRowsState,
  worktreeId: string
): RetainedAgentEntry[] {
  return getRetainedEntriesByWorktree(state).get(worktreeId) ?? EMPTY_RETAINED
}

export function selectRuntimeAgentOrchestrationForWorktree(
  state: RuntimeAgentOrchestrationState,
  worktreeId: string
): Record<string, AgentStatusOrchestrationContext> {
  return (
    getRuntimeAgentOrchestrationByWorktree(state).get(worktreeId) ??
    EMPTY_RUNTIME_AGENT_ORCHESTRATION
  )
}

export function selectTerminalLayoutsForWorktree(
  state: Pick<AppState, 'tabsByWorktree' | 'terminalLayoutsByTabId'>,
  worktreeId: string
): Record<string, TerminalLayoutSnapshot | undefined> {
  const out: Record<string, TerminalLayoutSnapshot | undefined> = {}
  for (const tab of (state.tabsByWorktree ?? EMPTY_RECORD)[worktreeId] ?? []) {
    out[tab.id] = (state.terminalLayoutsByTabId ?? EMPTY_RECORD)[tab.id]
  }
  return out
}
