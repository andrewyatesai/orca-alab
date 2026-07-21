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

// Why: this class is a thin delegating shim over the orca-runtime SQLite store
// (the `OrchestrationStore` napi class). The `node:sqlite` twin — schema,
// migrations, every query — was deleted; Rust is the sole implementation. The
// shim keeps only the JS-side nondeterminism the Rust store must NOT own so the
// bytes stay identical to the deleted TS store: generated ids, the two
// `new Date().toISOString()` completion stamps, and the UTF-16-aware display
// derivation. Everything else marshals through JSON (the store serializes each
// row to its TS Row shape). Row-returning getters map the store's `null`
// (absent row) back to `undefined` to preserve the old return contract.
export class OrchestrationDb {
  private store: RustOrchestrationStoreHandle

  constructor(dbPath: string | ':memory:') {
    // Lazy-require so merely importing this module never forces the native addon
    // to load — only an actual store instantiation depends on it.
    this.store = new (requireRustGitBinding().OrchestrationStore)(dbPath)
  }

  private static row<T>(json: string): T {
    return JSON.parse(json) as T
  }

  private static optRow<T>(json: string | null): T | undefined {
    return json === null ? undefined : (JSON.parse(json) as T)
  }

  private static list<T>(json: string): T[] {
    return JSON.parse(json) as T[]
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
  }): MessageRow {
    // senderPaneKey is the remint-stable pane identity persisted with the row so
    // worker_done/heartbeat lifecycle authority survives handle remints (v6 col).
    return OrchestrationDb.row<MessageRow>(
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
        msg.senderPaneKey ?? null
      )
    )
  }

  getUnreadMessages(toHandle: string, types?: MessageType[]): MessageRow[] {
    return OrchestrationDb.list<MessageRow>(
      this.store.getUnreadMessages(toHandle, types && types.length > 0 ? types : undefined)
    )
  }

  // Why: rewrites a superseded worker_done/heartbeat into a high-priority
  // rejection (subject/body/payload marker) so it stays auditable but is never
  // read back as an actionable completion/liveness signal. The marker
  // construction is deterministic, so it lives in the Rust store, not here.
  convertLifecycleMessageToRejection(messageId: string, reason: string): MessageRow | undefined {
    return OrchestrationDb.optRow<MessageRow>(
      this.store.convertLifecycleMessageToRejection(messageId, reason)
    )
  }

  getUndeliveredUnreadMessages(toHandle: string, types?: MessageType[]): MessageRow[] {
    return OrchestrationDb.list<MessageRow>(
      this.store.getUndeliveredUnreadMessages(
        toHandle,
        types && types.length > 0 ? types : undefined
      )
    )
  }

  getAllMessages(toHandle: string, limit = 20): MessageRow[] {
    return OrchestrationDb.list<MessageRow>(this.store.getAllMessages(toHandle, limit))
  }

  getMessageById(id: string): MessageRow | undefined {
    return OrchestrationDb.optRow<MessageRow>(this.store.getMessageById(id))
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
    return OrchestrationDb.list<MessageRow>(this.store.getInbox(limit))
  }

  getAllMessagesForHandle(toHandle: string, limit = 100, types?: MessageType[]): MessageRow[] {
    return OrchestrationDb.list<MessageRow>(
      this.store.getAllMessagesForHandle(
        toHandle,
        limit,
        types && types.length > 0 ? types : undefined
      )
    )
  }

  getThreadMessagesFor(threadId: string, toHandle: string, afterSequence?: number): MessageRow[] {
    return OrchestrationDb.list<MessageRow>(
      this.store.getThreadMessagesFor(threadId, toHandle, afterSequence)
    )
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
    return OrchestrationDb.row<TaskRow>(
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
    return OrchestrationDb.optRow<TaskRow>(this.store.getTask(id))
  }

  listTasks(filter?: { status?: TaskStatus; ready?: boolean }): TaskRow[] {
    const status = filter?.ready ? 'ready' : filter?.status
    return OrchestrationDb.list<TaskRow>(this.store.listTasks(status))
  }

  listTasksWithDispatch(filter?: { status?: TaskStatus; ready?: boolean }): TaskWithDispatchRow[] {
    const status = filter?.ready ? 'ready' : filter?.status
    return OrchestrationDb.list<TaskWithDispatchRow>(this.store.listTasksWithDispatch(status))
  }

  updateTaskStatus(id: string, status: TaskStatus, result?: string): TaskRow | undefined {
    // The exact ISO completion stamp is minted here (not in SQL) so it is
    // byte-identical to what the deleted TS store wrote.
    const completedAt =
      status === 'completed' || status === 'failed' ? new Date().toISOString() : null
    return OrchestrationDb.optRow<TaskRow>(
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
    return OrchestrationDb.row<DispatchContextRow>(
      this.store.createDispatchContext(
        taskId,
        assigneeHandle,
        generateId('ctx'),
        assigneePaneKey ?? null
      )
    )
  }

  getDispatchContext(taskId: string): DispatchContextRow | undefined {
    return OrchestrationDb.optRow<DispatchContextRow>(this.store.getDispatchContext(taskId))
  }

  getDispatchContextById(dispatchId: string): DispatchContextRow | undefined {
    return OrchestrationDb.optRow<DispatchContextRow>(this.store.getDispatchContextById(dispatchId))
  }

  getActiveDispatchForTerminal(handle: string): DispatchContextRow | undefined {
    return OrchestrationDb.optRow<DispatchContextRow>(
      this.store.getActiveDispatchForTerminal(handle)
    )
  }

  getLatestDispatchForTerminal(handle: string): DispatchContextRow | undefined {
    return OrchestrationDb.optRow<DispatchContextRow>(
      this.store.getLatestDispatchForTerminal(handle)
    )
  }

  completeDispatch(ctxId: string): void {
    this.store.completeDispatch(ctxId)
  }

  completeActiveDispatchForTask(taskId: string): void {
    this.store.completeActiveDispatchForTask(taskId)
  }

  failActiveDispatchForTask(taskId: string, error: string): DispatchContextRow | undefined {
    return OrchestrationDb.optRow<DispatchContextRow>(
      this.store.failActiveDispatchForTask(taskId, error)
    )
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
    return OrchestrationDb.list<DispatchContextRow>(this.store.getStaleDispatches(thresholdIso))
  }

  failDispatch(ctxId: string, error: string): DispatchContextRow | undefined {
    return OrchestrationDb.optRow<DispatchContextRow>(this.store.failDispatch(ctxId, error))
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
    return OrchestrationDb.row<DecisionGateRow>(
      this.store.createGate(generateId('gate'), gate.taskId, gate.question, gate.options ?? [])
    )
  }

  resolveGate(gateId: string, resolution: string): DecisionGateRow | undefined {
    return OrchestrationDb.optRow<DecisionGateRow>(this.store.resolveGate(gateId, resolution))
  }

  timeoutGate(gateId: string): DecisionGateRow | undefined {
    return OrchestrationDb.optRow<DecisionGateRow>(this.store.timeoutGate(gateId))
  }

  listGates(filter?: { taskId?: string; status?: GateStatus }): DecisionGateRow[] {
    return OrchestrationDb.list<DecisionGateRow>(
      this.store.listGates(filter?.taskId, filter?.status)
    )
  }

  getGate(id: string): DecisionGateRow | undefined {
    return OrchestrationDb.optRow<DecisionGateRow>(this.store.getGate(id))
  }

  // ── Coordinator Runs ──

  createCoordinatorRun(run: {
    spec: string
    coordinatorHandle: string
    pollIntervalMs?: number
  }): CoordinatorRun {
    return OrchestrationDb.row<CoordinatorRun>(
      this.store.createCoordinatorRun(
        generateId('run'),
        run.spec,
        run.coordinatorHandle,
        run.pollIntervalMs
      )
    )
  }

  getCoordinatorRun(id: string): CoordinatorRun | undefined {
    return OrchestrationDb.optRow<CoordinatorRun>(this.store.getCoordinatorRun(id))
  }

  updateCoordinatorRun(id: string, status: CoordinatorStatus): CoordinatorRun | undefined {
    const completedAt =
      status === 'completed' || status === 'failed' ? new Date().toISOString() : null
    return OrchestrationDb.optRow<CoordinatorRun>(
      this.store.updateCoordinatorRun(id, status, completedAt)
    )
  }

  getActiveCoordinatorRun(): CoordinatorRun | undefined {
    return OrchestrationDb.optRow<CoordinatorRun>(this.store.getActiveCoordinatorRun())
  }

  // Why: orchestrators may run concurrently (#4389) — gating needs every running row.
  getActiveCoordinatorRuns(): CoordinatorRun[] {
    return OrchestrationDb.list<CoordinatorRun>(this.store.getActiveCoordinatorRuns())
  }

  // ── Queries for Coordinator ──

  getIdleTerminals(excludeHandles: string[] = []): string[] {
    return OrchestrationDb.list<string>(this.store.getIdleTerminals(excludeHandles))
  }

  // ── Lifecycle ──

  resetAll(): void {
    this.store.resetAll()
  }

  resetTasks(): void {
    this.store.resetTasks()
  }

  resetMessages(): void {
    this.store.resetMessages()
  }

  close(): void {
    this.store.close()
  }
}
