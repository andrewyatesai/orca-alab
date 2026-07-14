import type { RetainedAgentEntry } from '@/store/slices/agent-status'
import type { AppState } from '@/store/types'
import type {
  AgentStatusEntry,
  AgentStatusOrchestrationContext,
  MigrationUnsupportedPtyEntry
} from '../../../../shared/agent-status-types'
import { parsePaneKey } from '../../../../shared/stable-pane-id'

// Why: selector unit tests often pass partial store mocks; production state
// owns these maps, but missing mock maps should behave like empty slices.
const EMPTY_RECORD = {}

export type WorktreeAgentRowsState = Pick<
  AppState,
  | 'agentStatusByPaneKey'
  | 'migrationUnsupportedByPtyId'
  | 'retainedAgentsByPaneKey'
  | 'tabsByWorktree'
>

export type RuntimeAgentOrchestrationState = Pick<
  AppState,
  | 'agentStatusByPaneKey'
  | 'retainedAgentsByPaneKey'
  | 'runtimeAgentOrchestrationByPaneKey'
  | 'tabsByWorktree'
>

type TabWorktreeIndexCache = {
  tabsByWorktree: WorktreeAgentRowsState['tabsByWorktree']
  tabIdToWorktreeId: Map<string, string>
}

type LiveEntriesByWorktreeCache = {
  tabsByWorktree: WorktreeAgentRowsState['tabsByWorktree']
  agentStatusByPaneKey: WorktreeAgentRowsState['agentStatusByPaneKey']
  entriesByWorktree: Map<string, AgentStatusEntry[]>
}

type MigrationUnsupportedByWorktreeCache = {
  tabsByWorktree: WorktreeAgentRowsState['tabsByWorktree']
  migrationUnsupportedByPtyId: WorktreeAgentRowsState['migrationUnsupportedByPtyId']
  entriesByWorktree: Map<string, MigrationUnsupportedPtyEntry[]>
}

type RetainedEntriesByWorktreeCache = {
  retainedAgentsByPaneKey: WorktreeAgentRowsState['retainedAgentsByPaneKey']
  entriesByWorktree: Map<string, RetainedAgentEntry[]>
}

type RuntimeAgentOrchestrationByWorktreeCache = {
  tabsByWorktree: RuntimeAgentOrchestrationState['tabsByWorktree']
  agentStatusByPaneKey: RuntimeAgentOrchestrationState['agentStatusByPaneKey']
  retainedAgentsByPaneKey: RuntimeAgentOrchestrationState['retainedAgentsByPaneKey']
  runtimeAgentOrchestrationByPaneKey: RuntimeAgentOrchestrationState['runtimeAgentOrchestrationByPaneKey']
  orchestrationByWorktree: Map<string, Record<string, AgentStatusOrchestrationContext>>
}

let tabWorktreeIndexCache: TabWorktreeIndexCache | null = null
let liveEntriesByWorktreeCache: LiveEntriesByWorktreeCache | null = null
let migrationUnsupportedByWorktreeCache: MigrationUnsupportedByWorktreeCache | null = null
let retainedEntriesByWorktreeCache: RetainedEntriesByWorktreeCache | null = null
let runtimeAgentOrchestrationByWorktreeCache: RuntimeAgentOrchestrationByWorktreeCache | null = null

function reuseArrayIfEqual<T>(previous: T[] | undefined, next: T[]): T[] {
  if (!previous || previous.length !== next.length) {
    return next
  }
  for (let i = 0; i < next.length; i += 1) {
    if (previous[i] !== next[i]) {
      return next
    }
  }
  return previous
}

// Why: the runtime-orchestration index returns per-worktree Records (not
// arrays); mirror reuseArrayIfEqual so a worktree unaffected by a write keeps
// its reference and useShallow keeps suppressing its card's re-render.
function reuseRecordIfEqual<T>(
  previous: Record<string, T> | undefined,
  next: Record<string, T>
): Record<string, T> {
  if (!previous) {
    return next
  }
  const previousKeys = Object.keys(previous)
  const nextKeys = Object.keys(next)
  if (previousKeys.length !== nextKeys.length) {
    return next
  }
  for (const key of nextKeys) {
    if (previous[key] !== next[key]) {
      return next
    }
  }
  return previous
}

function getTabIdToWorktreeId(
  tabsByWorktree: WorktreeAgentRowsState['tabsByWorktree']
): Map<string, string> {
  if (tabWorktreeIndexCache?.tabsByWorktree === tabsByWorktree) {
    return tabWorktreeIndexCache.tabIdToWorktreeId
  }
  const tabIdToWorktreeId = new Map<string, string>()
  for (const [worktreeId, tabs] of Object.entries(tabsByWorktree)) {
    for (const tab of tabs) {
      tabIdToWorktreeId.set(tab.id, worktreeId)
    }
  }
  tabWorktreeIndexCache = { tabsByWorktree, tabIdToWorktreeId }
  return tabIdToWorktreeId
}

export function getLiveEntriesByWorktree(
  state: WorktreeAgentRowsState
): Map<string, AgentStatusEntry[]> {
  const agentStatusByPaneKey = state.agentStatusByPaneKey ?? EMPTY_RECORD
  const tabsByWorktree = state.tabsByWorktree ?? EMPTY_RECORD
  if (
    liveEntriesByWorktreeCache?.tabsByWorktree === tabsByWorktree &&
    liveEntriesByWorktreeCache.agentStatusByPaneKey === agentStatusByPaneKey
  ) {
    return liveEntriesByWorktreeCache.entriesByWorktree
  }

  const tabIdToWorktreeId = getTabIdToWorktreeId(tabsByWorktree)
  const previous = liveEntriesByWorktreeCache?.entriesByWorktree
  const entriesByWorktree = new Map<string, AgentStatusEntry[]>()
  for (const [paneKey, entry] of Object.entries(agentStatusByPaneKey)) {
    const parsed = parsePaneKey(paneKey)
    if (!parsed) {
      continue
    }
    const tabWorktreeId = tabIdToWorktreeId.get(parsed.tabId)
    // Why: keep early attributed child rows, but hide completed rows once their tab is gone.
    const worktreeId = tabWorktreeId ?? (entry.state === 'done' ? undefined : entry.worktreeId)
    if (!worktreeId) {
      continue
    }
    const bucket = entriesByWorktree.get(worktreeId)
    if (bucket) {
      bucket.push(entry)
    } else {
      entriesByWorktree.set(worktreeId, [entry])
    }
  }
  for (const [worktreeId, entries] of entriesByWorktree) {
    entriesByWorktree.set(worktreeId, reuseArrayIfEqual(previous?.get(worktreeId), entries))
  }
  liveEntriesByWorktreeCache = {
    tabsByWorktree,
    agentStatusByPaneKey,
    entriesByWorktree
  }
  return entriesByWorktree
}

export function getMigrationUnsupportedByWorktree(
  state: WorktreeAgentRowsState
): Map<string, MigrationUnsupportedPtyEntry[]> {
  const migrationUnsupportedByPtyId = state.migrationUnsupportedByPtyId ?? EMPTY_RECORD
  const tabsByWorktree = state.tabsByWorktree ?? EMPTY_RECORD
  if (
    migrationUnsupportedByWorktreeCache?.tabsByWorktree === tabsByWorktree &&
    migrationUnsupportedByWorktreeCache.migrationUnsupportedByPtyId === migrationUnsupportedByPtyId
  ) {
    return migrationUnsupportedByWorktreeCache.entriesByWorktree
  }

  const tabIdToWorktreeId = getTabIdToWorktreeId(tabsByWorktree)
  const previous = migrationUnsupportedByWorktreeCache?.entriesByWorktree
  const entriesByWorktree = new Map<string, MigrationUnsupportedPtyEntry[]>()
  for (const unsupported of Object.values(migrationUnsupportedByPtyId)) {
    if (!unsupported.paneKey) {
      continue
    }
    const parsed = parsePaneKey(unsupported.paneKey)
    const worktreeId = parsed ? tabIdToWorktreeId.get(parsed.tabId) : undefined
    if (!worktreeId) {
      continue
    }
    const bucket = entriesByWorktree.get(worktreeId)
    if (bucket) {
      bucket.push(unsupported)
    } else {
      entriesByWorktree.set(worktreeId, [unsupported])
    }
  }
  for (const [worktreeId, entries] of entriesByWorktree) {
    entriesByWorktree.set(worktreeId, reuseArrayIfEqual(previous?.get(worktreeId), entries))
  }
  migrationUnsupportedByWorktreeCache = {
    tabsByWorktree,
    migrationUnsupportedByPtyId,
    entriesByWorktree
  }
  return entriesByWorktree
}

export function getRetainedEntriesByWorktree(
  state: WorktreeAgentRowsState
): Map<string, RetainedAgentEntry[]> {
  const retainedAgentsByPaneKey = state.retainedAgentsByPaneKey ?? EMPTY_RECORD
  if (retainedEntriesByWorktreeCache?.retainedAgentsByPaneKey === retainedAgentsByPaneKey) {
    return retainedEntriesByWorktreeCache.entriesByWorktree
  }

  const previous = retainedEntriesByWorktreeCache?.entriesByWorktree
  const entriesByWorktree = new Map<string, RetainedAgentEntry[]>()
  for (const retained of Object.values(retainedAgentsByPaneKey)) {
    const bucket = entriesByWorktree.get(retained.worktreeId)
    if (bucket) {
      bucket.push(retained)
    } else {
      entriesByWorktree.set(retained.worktreeId, [retained])
    }
  }
  for (const [worktreeId, entries] of entriesByWorktree) {
    entriesByWorktree.set(worktreeId, reuseArrayIfEqual(previous?.get(worktreeId), entries))
  }
  retainedEntriesByWorktreeCache = {
    retainedAgentsByPaneKey,
    entriesByWorktree
  }
  return entriesByWorktree
}

export function getRuntimeAgentOrchestrationByWorktree(
  state: RuntimeAgentOrchestrationState
): Map<string, Record<string, AgentStatusOrchestrationContext>> {
  const runtimeAgentOrchestrationByPaneKey =
    state.runtimeAgentOrchestrationByPaneKey ?? EMPTY_RECORD
  const agentStatusByPaneKey = state.agentStatusByPaneKey ?? EMPTY_RECORD
  const retainedAgentsByPaneKey = state.retainedAgentsByPaneKey ?? EMPTY_RECORD
  const tabsByWorktree = state.tabsByWorktree ?? EMPTY_RECORD
  if (
    runtimeAgentOrchestrationByWorktreeCache?.runtimeAgentOrchestrationByPaneKey ===
      runtimeAgentOrchestrationByPaneKey &&
    runtimeAgentOrchestrationByWorktreeCache.agentStatusByPaneKey === agentStatusByPaneKey &&
    runtimeAgentOrchestrationByWorktreeCache.retainedAgentsByPaneKey === retainedAgentsByPaneKey &&
    runtimeAgentOrchestrationByWorktreeCache.tabsByWorktree === tabsByWorktree
  ) {
    return runtimeAgentOrchestrationByWorktreeCache.orchestrationByWorktree
  }

  const tabIdToWorktreeId = getTabIdToWorktreeId(tabsByWorktree)
  const previous = runtimeAgentOrchestrationByWorktreeCache?.orchestrationByWorktree
  const orchestrationByWorktree = new Map<
    string,
    Record<string, AgentStatusOrchestrationContext>
  >()
  for (const [paneKey, orchestration] of Object.entries(runtimeAgentOrchestrationByPaneKey)) {
    const parsed = parsePaneKey(paneKey)
    const parsedParent = orchestration.parentPaneKey
      ? parsePaneKey(orchestration.parentPaneKey)
      : null
    const liveEntry = agentStatusByPaneKey[paneKey]
    const retainedEntry = retainedAgentsByPaneKey[paneKey]
    // Why: child agent terminals can be attributed to a worktree before their
    // tab reaches this renderer, or after the row has been retained as done.
    // One entry can match several worktrees (own tab, parent tab, live/retained
    // attribution); index it under each so every card that matched before still
    // sees it — mirroring the per-worktree OR the selector used to evaluate.
    const ownWorktreeId = parsed ? tabIdToWorktreeId.get(parsed.tabId) : undefined
    const parentWorktreeId = parsedParent ? tabIdToWorktreeId.get(parsedParent.tabId) : undefined
    const worktreeIds = new Set<string>()
    for (const worktreeId of [
      ownWorktreeId,
      parentWorktreeId,
      liveEntry?.worktreeId,
      retainedEntry?.worktreeId
    ]) {
      if (worktreeId) {
        worktreeIds.add(worktreeId)
      }
    }
    for (const worktreeId of worktreeIds) {
      const bucket = orchestrationByWorktree.get(worktreeId)
      if (bucket) {
        bucket[paneKey] = orchestration
      } else {
        orchestrationByWorktree.set(worktreeId, { [paneKey]: orchestration })
      }
    }
  }
  for (const [worktreeId, orchestration] of orchestrationByWorktree) {
    orchestrationByWorktree.set(
      worktreeId,
      reuseRecordIfEqual(previous?.get(worktreeId), orchestration)
    )
  }
  runtimeAgentOrchestrationByWorktreeCache = {
    tabsByWorktree,
    agentStatusByPaneKey,
    retainedAgentsByPaneKey,
    runtimeAgentOrchestrationByPaneKey,
    orchestrationByWorktree
  }
  return orchestrationByWorktree
}
