import { AGENT_STATUS_MAX_SUBAGENTS } from './agent-status-types'
import {
  isClaudeTeammateLifecycleId,
  upsertWorkingClaudeSubagent,
  type ClaudeSubagentRoster
} from './claude-subagent-roster'

/** One agent entry from the `background_tasks` array Claude attaches to Stop
 *  (and SubagentStop) hook payloads. Non-agent task types (background shells,
 *  crons) are filtered out at read time. */
export type ClaudeBackgroundAgentTask = {
  id: string
  agentType?: string
  description?: string
  running: boolean
  /** True for `type: "teammate"` entries. Their ids never match lifecycle
   *  agent_ids and they report "running" permanently — even after the named
   *  agent finished — so they carry no per-agent state at all. */
  teammate: boolean
}

/** Read the agent-typed entries of a hook payload's `background_tasks` field.
 *  `present: false` means the field was absent/malformed (older Claude builds),
 *  so callers must keep their tracked roster instead of clearing it. */
export function readClaudeBackgroundAgentTasks(hookPayload: Record<string, unknown>): {
  present: boolean
  tasks: ClaudeBackgroundAgentTask[]
  truncated: boolean
} {
  const raw = hookPayload['background_tasks']
  if (!Array.isArray(raw)) {
    return { present: false, tasks: [], truncated: false }
  }
  const tasks: ClaudeBackgroundAgentTask[] = []
  let truncated = false
  for (const item of raw) {
    if (typeof item !== 'object' || item === null) {
      continue
    }
    const obj = item as Record<string, unknown>
    if (obj.type !== 'subagent' && obj.type !== 'teammate') {
      continue
    }
    if (typeof obj.id !== 'string' || obj.id.trim().length === 0) {
      continue
    }
    if (tasks.length >= AGENT_STATUS_MAX_SUBAGENTS) {
      // Why: a capped inventory cannot prove a tracked id is absent; callers
      // must retain unlisted rows rather than deleting live overflow tasks.
      truncated = true
      break
    }
    tasks.push({
      id: obj.id,
      agentType: typeof obj.agent_type === 'string' ? obj.agent_type : undefined,
      description: typeof obj.description === 'string' ? obj.description : undefined,
      running: obj.status === 'running',
      teammate: obj.type === 'teammate'
    })
  }
  return { present: true, tasks, truncated }
}

/** Fold a lead Stop's `background_tasks` into the lifecycle-tracked roster.
 *
 *  The list is authoritative for subagent-typed entries only: a running
 *  one-shot/workflow lane is always listed under its lifecycle `agent_id`,
 *  foreground children cannot span a lead Stop, and finished tasks drop from
 *  the list. Teammate-typed entries prove nothing per-agent (unrelated ids,
 *  permanently "running") — but their PRESENCE proves the session has
 *  named-agent/teammate machinery, and their total absence from a complete
 *  inventory proves no teammate-shaped child can still be alive. So:
 *  - an empty list proves nothing is left alive → clear the roster;
 *  - an id-exact subagent-typed match that is running is trusted fully and
 *    tagged listedAsSubagentTask; one reported not running is removed;
 *  - an unmatched RUNNING subagent-typed entry is a one-shot this listener
 *    never saw start (Orca/relay restart mid-run) → recreate it;
 *  - an unlisted entry is finished or dead (its SubagentStop was lost) →
 *    remove it — UNLESS it is teammate-shaped, live-tracked, never
 *    subagent-listed, the list still shows teammate-typed tasks, AND it is
 *    working or TeammateIdle-confirmed: that is a named teammate whose id
 *    simply never appears, and removing it would drop the pane's done-gate
 *    (working) or its parked idle row (confirmed). */
export function foldClaudeBackgroundTasksIntoRoster(
  roster: ClaudeSubagentRoster,
  tasks: ClaudeBackgroundAgentTask[],
  now: number,
  options?: { inventoryComplete?: boolean }
): void {
  if (tasks.length === 0) {
    if (options?.inventoryComplete !== false) {
      roster.clear()
    }
    return
  }
  const listedIds = new Set<string>()
  const pendingRunningTasks = new Map<string, ClaudeBackgroundAgentTask>()
  const hasTeammateTypedTask = tasks.some((task) => task.teammate)
  for (const task of tasks) {
    if (task.teammate) {
      continue
    }
    listedIds.add(task.id)
    const existing = roster.get(task.id)
    if (existing) {
      if (!task.running) {
        roster.delete(task.id)
        pendingRunningTasks.delete(task.id)
        continue
      }
      // Why: a Stop can park the row before the lead inventory confirms the
      // same workflow lane is still running; the authoritative task wins.
      existing.state = 'working'
      existing.agentType = task.agentType ?? existing.agentType
      existing.description = task.description ?? existing.description
      existing.listedAsSubagentTask = true
      continue
    }
    if (!task.running) {
      pendingRunningTasks.delete(task.id)
      continue
    }
    upsertWorkingClaudeSubagent(
      roster,
      task.id,
      { agentType: task.agentType, description: task.description },
      now
    )
    const created = roster.get(task.id)
    if (created) {
      created.backgroundTasksAuthoritative = true
      created.listedAsSubagentTask = true
    } else {
      // Why: a full roster may still contain stale entries that this same
      // inventory will reap. Retry after cleanup so a replacement stays live.
      pendingRunningTasks.set(task.id, task)
    }
  }
  if (options?.inventoryComplete !== false) {
    for (const [id, tracked] of roster) {
      if (listedIds.has(id)) {
        continue
      }
      if (
        hasTeammateTypedTask &&
        !tracked.backgroundTasksAuthoritative &&
        tracked.listedAsSubagentTask !== true &&
        isClaudeTeammateLifecycleId(id) &&
        // Why: an idle row that no TeammateIdle ever confirmed is a finished
        // workflow lane wearing a teammate-shaped id — reap it here or the
        // pre-#8825 idle pile rebuilds one lane per lead turn.
        (tracked.state === 'working' || tracked.confirmedTeammate === true)
      ) {
        continue
      }
      roster.delete(id)
    }
  }
  for (const task of pendingRunningTasks.values()) {
    if (roster.size >= AGENT_STATUS_MAX_SUBAGENTS) {
      break
    }
    upsertWorkingClaudeSubagent(
      roster,
      task.id,
      { agentType: task.agentType, description: task.description },
      now
    )
    const created = roster.get(task.id)
    if (created) {
      created.backgroundTasksAuthoritative = true
      created.listedAsSubagentTask = true
    }
  }
}
