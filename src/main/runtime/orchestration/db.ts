import { randomBytes } from 'node:crypto'
import {
  requireRustGitBinding,
  type RustOrchestrationStoreHandle
} from '../../daemon/rust-git-addon'
import type {
  MessageType,
  MessagePriority,
  TaskStatus,
  DispatchStatus,
  GateStatus,
  CoordinatorStatus,
  MessageRow,
  TaskRow,
  DispatchContextRow,
  DecisionGateRow,
  CoordinatorRun
} from './types'
import { buildOrchestrationTaskDisplayMetadata } from '../../../shared/orchestration-task-display'
import {
  messageListFromJson,
  messageRowFromJson,
  optionalMessageRowFromJson
} from './db-message-timestamp'
import { listFromJson, optRowFromJson, rowFromJson } from './db-row-json'

export type {
  MessageType,
  MessagePriority,
  TaskStatus,
  DispatchStatus,
  GateStatus,
  CoordinatorStatus,
  MessageRow,
  TaskRow,
  DispatchContextRow,
  DecisionGateRow,
  CoordinatorRun
}

// The join shape returned by listTasksWithDispatch: a task row plus the active
// dispatch's assignee/id (or null when the task has no live dispatch).
type TaskWithDispatchRow = TaskRow & { assignee_handle: string | null; dispatch_id: string | null }

// Ids stay `<prefix>_<hex>` (the shim owns generation, not Rust): orca-runtime.ts
// extracts task ids with `/task_[A-Za-z0-9]+/`, so the format is a contract.
function generateId(prefix: string): string {
  return `${prefix}_${randomBytes(6).toString('hex')}`
}

// Why: the store treats an empty filter as "no filter"; normalize before crossing napi.
function typesFilter(types?: MessageType[]): MessageType[] | undefined {
  return types && types.length > 0 ? types : undefined
}

// Why: this class is a thin delegating shim over the orca-runtime SQLite store
// (the `OrchestrationStore` napi class). The `node:sqlite` twin — schema,
// migrations, every query — was deleted; Rust is the sole implementation. The
// shim keeps only the JS-side nondeterminism the Rust store must NOT own so the
// bytes stay identical to the deleted TS store: generated ids, the two
// `new Date().toISOString()` completion stamps, the UTF-16-aware display
// derivation, and the RFC3339 exposure of message timestamps (see
// db-message-timestamp.ts). Everything else marshals through JSON
// (the store serializes each row to its TS Row shape). Row-returning getters map
// the store's `null` (absent row) back to `undefined` to preserve the old
// return contract.
export class OrchestrationDb {
  private store: RustOrchestrationStoreHandle

  // Why: buildAgentOrchestrationByPaneKey rebuilds context on every 16ms graph
  // publish, issuing ~2 napi dispatch lookups per terminal. The overwhelming
  // majority never orchestrate, so cache "any dispatch rows exist?" to let the
  // builder short-circuit the whole fan-out (#9694). createDispatchContext flips
  // it true; resets clear it back to a cold re-derive.
  private hasAnyDispatchContextsCache: boolean | undefined

  constructor(dbPath: string | ':memory:') {
    // Lazy-require so merely importing this module never forces the native addon
    // to load — only an actual store instantiation depends on it.
    this.store = new (requireRustGitBinding().OrchestrationStore)(dbPath)
  }

  // ── Messages ──

  insertMessage(msg: {
    from: string
    to: string
    subject: string
    body?: string
    type?: MessageType
    priority?: MessagePriority
    threadId?: string
    payload?: string
    senderPaneKey?: string
    recipientPaneKey?: string
  }): MessageRow {
    // senderPaneKey is the remint-stable pane identity persisted with the row so
    // worker_done/heartbeat lifecycle authority survives handle remints (v6 col).
    // recipientPaneKey lets delivery follow the pane after the addressed handle
    // goes stale (#9163, v7 col).
    return messageRowFromJson(
      this.store.insertMessage(
        generateId('msg'),
        msg.from,
        msg.to,
        msg.subject,
        msg.body ?? '',
        msg.type ?? 'status',
        msg.priority ?? 'normal',
        msg.threadId ?? null,
        msg.payload ?? null,
        msg.senderPaneKey ?? null,
        msg.recipientPaneKey ?? null
      )
    )
  }

  getUnreadMessages(toHandle: string, types?: MessageType[]): MessageRow[] {
    return messageListFromJson(this.store.getUnreadMessages(toHandle, typesFilter(types)))
  }

  // Why: rewrites a superseded worker_done/heartbeat into a high-priority
  // rejection (subject/body/payload marker) so it stays auditable but is never
  // read back as an actionable completion/liveness signal. The marker
  // construction is deterministic, so it lives in the Rust store, not here.
  convertLifecycleMessageToRejection(messageId: string, reason: string): MessageRow | undefined {
    return optionalMessageRowFromJson(
      this.store.convertLifecycleMessageToRejection(messageId, reason)
    )
  }

  getUndeliveredUnreadMessages(toHandle: string, types?: MessageType[]): MessageRow[] {
    return messageListFromJson(
      this.store.getUndeliveredUnreadMessages(toHandle, typesFilter(types))
    )
  }

  getAllMessages(toHandle: string, limit = 20): MessageRow[] {
    return messageListFromJson(this.store.getAllMessages(toHandle, limit))
  }

  getMessageById(id: string): MessageRow | undefined {
    return optionalMessageRowFromJson(this.store.getMessageById(id))
  }

  markAsRead(ids: string[]): void {
    if (ids.length === 0) {
      return
    }
    this.store.markAsRead(ids)
  }

  markAsDelivered(ids: string[]): void {
    if (ids.length === 0) {
      return
    }
    this.store.markAsDelivered(ids)
  }

  // Why: superseded lifecycle messages stay queryable through history but must
  // not be consumed or injected after their dispatch has finished. The store
  // preserves an existing delivered_at (COALESCE) rather than restamping it.
  markAsReadAndDelivered(ids: string[]): void {
    if (ids.length === 0) {
      return
    }
    this.store.markAsReadAndDelivered(ids)
  }

  getInbox(limit = 20): MessageRow[] {
    return messageListFromJson(this.store.getInbox(limit))
  }

  getAllMessagesForHandle(toHandle: string, limit = 100, types?: MessageType[]): MessageRow[] {
    return messageListFromJson(
      this.store.getAllMessagesForHandle(toHandle, limit, typesFilter(types))
    )
  }

  getThreadMessagesFor(threadId: string, toHandle: string, afterSequence?: number): MessageRow[] {
    return messageListFromJson(this.store.getThreadMessagesFor(threadId, toHandle, afterSequence))
  }

  // ── Tasks ──

  createTask(task: {
    spec: string
    taskTitle?: string
    displayName?: string
    deps?: string[]
    parentId?: string
    createdByTerminalHandle?: string
  }): TaskRow {
    // The UTF-16-aware label derivation stays in JS; the resolved strings are
    // passed to the store so Rust needs no port of it.
    const display = buildOrchestrationTaskDisplayMetadata({
      spec: task.spec,
      taskTitle: task.taskTitle,
      displayName: task.displayName
    })
    return rowFromJson<TaskRow>(
      this.store.createTask(
        generateId('task'),
        task.spec,
        task.parentId ?? null,
        task.deps ?? [],
        task.createdByTerminalHandle ?? null,
        display.taskTitle || null,
        display.displayName || null
      )
    )
  }

  getTask(id: string): TaskRow | undefined {
    return optRowFromJson<TaskRow>(this.store.getTask(id))
  }

  listTasks(filter?: { status?: TaskStatus; ready?: boolean }): TaskRow[] {
    const status = filter?.ready ? 'ready' : filter?.status
    return listFromJson<TaskRow>(this.store.listTasks(status))
  }

  listTasksWithDispatch(filter?: { status?: TaskStatus; ready?: boolean }): TaskWithDispatchRow[] {
    const status = filter?.ready ? 'ready' : filter?.status
    return listFromJson<TaskWithDispatchRow>(this.store.listTasksWithDispatch(status))
  }

  updateTaskStatus(id: string, status: TaskStatus, result?: string): TaskRow | undefined {
    // The exact ISO completion stamp is minted here (not in SQL) so it is
    // byte-identical to what the deleted TS store wrote.
    const completedAt =
      status === 'completed' || status === 'failed' ? new Date().toISOString() : null
    return optRowFromJson<TaskRow>(
      this.store.updateTaskStatus(id, status, result ?? null, completedAt)
    )
  }

  // ── Dispatch Contexts ──

  createDispatchContext(
    taskId: string,
    assigneeHandle: string,
    // Why: the pane key is the remint-stable identity behind the handle;
    // recording it at dispatch time lets the store lock out a reminted handle
    // reopening a second concurrent dispatch on the same pane (v6 col).
    assigneePaneKey?: string
  ): DispatchContextRow {
    // The store throws the same guard-path messages the TS twin did
    // (`Task not found: …`, `… is <status>; only ready …`, `Terminal … already
    // has an active dispatch (… for task …)`) — consumers match on `.message`.
    const row = rowFromJson<DispatchContextRow>(
      this.store.createDispatchContext(
        taskId,
        assigneeHandle,
        generateId('ctx'),
        assigneePaneKey ?? null
      )
    )
    this.hasAnyDispatchContextsCache = true
    return row
  }

  /**
   * Cheap "could any terminal have an active or recent-completed dispatch?"
   * probe. When false, orchestration-context builders skip their per-terminal
   * query fan-out entirely (#9694). Cached after first probe.
   *
   * Why the task-emptiness derivation: the Rust store exposes no direct
   * dispatch_contexts existence probe, and listTasksWithDispatch surfaces only
   * the ACTIVE dispatch id (null for a persisted *completed* dispatch), so it
   * would wrongly report empty on a cold DB whose only dispatch has finished —
   * dropping recent-completed context. A dispatch always references a task, so
   * "no tasks at all" safely implies "no dispatches" (the never-orchestrate
   * majority). The rare tasks-without-dispatch cold case just skips the win.
   */
  hasAnyDispatchContexts(): boolean {
    return (this.hasAnyDispatchContextsCache ??= this.listTasks().length > 0)
  }

  getDispatchContext(taskId: string): DispatchContextRow | undefined {
    return optRowFromJson<DispatchContextRow>(this.store.getDispatchContext(taskId))
  }

  getDispatchContextById(dispatchId: string): DispatchContextRow | undefined {
    return optRowFromJson<DispatchContextRow>(this.store.getDispatchContextById(dispatchId))
  }

  getActiveDispatchForTerminal(handle: string): DispatchContextRow | undefined {
    return optRowFromJson<DispatchContextRow>(this.store.getActiveDispatchForTerminal(handle))
  }

  getLatestDispatchForTerminal(handle: string): DispatchContextRow | undefined {
    return optRowFromJson<DispatchContextRow>(this.store.getLatestDispatchForTerminal(handle))
  }

  completeDispatch(ctxId: string): void {
    this.store.completeDispatch(ctxId)
  }

  completeActiveDispatchForTask(taskId: string): void {
    this.store.completeActiveDispatchForTask(taskId)
  }

  failActiveDispatchForTask(taskId: string, error: string): DispatchContextRow | undefined {
    return optRowFromJson<DispatchContextRow>(this.store.failActiveDispatchForTask(taskId, error))
  }

  recordHeartbeat(dispatchId: string, at: string): void {
    this.store.recordHeartbeat(dispatchId, at)
  }

  getStaleDispatches(thresholdIso: string): DispatchContextRow[] {
    // Why: delegates to the Rust orca-runtime store, whose get_stale_dispatches
    // already carries the full #8452/#8514 fix (status='dispatched' + dispatched_at
    // grace + datetime()-wrapped comparison so space-format columns and ISO-Z
    // thresholds compare correctly). Upstream's TS julianday() reimplementation is
    // superseded by that Rust query.
    return listFromJson<DispatchContextRow>(this.store.getStaleDispatches(thresholdIso))
  }

  failDispatch(ctxId: string, error: string): DispatchContextRow | undefined {
    return optRowFromJson<DispatchContextRow>(this.store.failDispatch(ctxId, error))
  }

  // Backdate a dispatch's `dispatched_at` / `last_heartbeat_at` — the seam the
  // stale-dispatch tests use to reach into the grace window without sleeping.
  setDispatchTimestamps(
    dispatchId: string,
    dispatchedAt?: string | null,
    lastHeartbeatAt?: string | null
  ): void {
    this.store.setDispatchTimestamps(dispatchId, dispatchedAt ?? null, lastHeartbeatAt ?? null)
  }

  // ── Decision Gates ──

  createGate(gate: { taskId: string; question: string; options?: string[] }): DecisionGateRow {
    return rowFromJson<DecisionGateRow>(
      this.store.createGate(generateId('gate'), gate.taskId, gate.question, gate.options ?? [])
    )
  }

  resolveGate(gateId: string, resolution: string): DecisionGateRow | undefined {
    return optRowFromJson<DecisionGateRow>(this.store.resolveGate(gateId, resolution))
  }

  timeoutGate(gateId: string): DecisionGateRow | undefined {
    return optRowFromJson<DecisionGateRow>(this.store.timeoutGate(gateId))
  }

  listGates(filter?: { taskId?: string; status?: GateStatus }): DecisionGateRow[] {
    return listFromJson<DecisionGateRow>(this.store.listGates(filter?.taskId, filter?.status))
  }

  getGate(id: string): DecisionGateRow | undefined {
    return optRowFromJson<DecisionGateRow>(this.store.getGate(id))
  }

  // ── Coordinator Runs ──

  createCoordinatorRun(run: {
    spec: string
    coordinatorHandle: string
    pollIntervalMs?: number
  }): CoordinatorRun {
    return rowFromJson<CoordinatorRun>(
      this.store.createCoordinatorRun(
        generateId('run'),
        run.spec,
        run.coordinatorHandle,
        run.pollIntervalMs
      )
    )
  }

  getCoordinatorRun(id: string): CoordinatorRun | undefined {
    return optRowFromJson<CoordinatorRun>(this.store.getCoordinatorRun(id))
  }

  updateCoordinatorRun(id: string, status: CoordinatorStatus): CoordinatorRun | undefined {
    const completedAt =
      status === 'completed' || status === 'failed' ? new Date().toISOString() : null
    return optRowFromJson<CoordinatorRun>(this.store.updateCoordinatorRun(id, status, completedAt))
  }

  getActiveCoordinatorRun(): CoordinatorRun | undefined {
    return optRowFromJson<CoordinatorRun>(this.store.getActiveCoordinatorRun())
  }

  // Why: orchestrators may run concurrently (#4389) — gating needs every running row.
  getActiveCoordinatorRuns(): CoordinatorRun[] {
    return listFromJson<CoordinatorRun>(this.store.getActiveCoordinatorRuns())
  }

  // ── Queries for Coordinator ──

  getIdleTerminals(excludeHandles: string[] = []): string[] {
    return listFromJson<string>(this.store.getIdleTerminals(excludeHandles))
  }

  // ── Lifecycle ──

  resetAll(): void {
    this.store.resetAll()
    this.hasAnyDispatchContextsCache = undefined
  }

  resetTasks(): void {
    this.store.resetTasks()
    this.hasAnyDispatchContextsCache = undefined
  }

  resetMessages(): void {
    this.store.resetMessages()
  }

  close(): void {
    this.store.close()
  }
}
