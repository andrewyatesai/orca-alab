// Canonicalization for the orchestration-store parity harness: turns the raw,
// nondeterministic SQLite state (random ids, datetime('now') timestamps) into a
// stable shape comparable across TS and Rust. Generated ids become creation-
// ordinal placeholders (task_0, ctx_1…) via a rowid-order id map; timestamps
// normalize to null / "SET" (preserving "was it stamped?" without the value).
// Mirrors the Rust dumps/projections in modules/orchestration_store.rs.

export type Row = Record<string, unknown>
type RawStatement = { all: (...params: unknown[]) => Row[] }
export type RawDb = { prepare: (sql: string) => RawStatement }

// table → canonical id prefix, in the fixed order the id map is built.
const TABLE_PREFIX: readonly [string, string][] = [
  ['messages', 'msg'],
  ['tasks', 'task'],
  ['dispatch_contexts', 'ctx'],
  ['decision_gates', 'gate'],
  ['coordinator_runs', 'run']
]

export function dumpRawTables(db: RawDb): Record<string, Row[]> {
  const out: Record<string, Row[]> = {}
  for (const [table] of TABLE_PREFIX) {
    out[table] = db.prepare(`SELECT * FROM ${table} ORDER BY rowid`).all()
  }
  return out
}

export function buildIdMap(tables: Record<string, Row[]>): Map<string, string> {
  const map = new Map<string, string>()
  for (const [table, prefix] of TABLE_PREFIX) {
    tables[table].forEach((row, index) => map.set(String(row.id), `${prefix}_${index}`))
  }
  return map
}

const canonId = (map: Map<string, string>, value: unknown): unknown =>
  typeof value === 'string' && map.has(value) ? map.get(value) : value
const normTs = (value: unknown): unknown => (value == null ? null : 'SET')
const canonDeps = (map: Map<string, string>, depsJson: unknown): unknown[] =>
  (JSON.parse(String(depsJson)) as string[]).map((dep) => map.get(dep) ?? dep)

// Sort a canonicalized read list by creation ordinal (the integer in the
// canonical id) so the order is deterministic regardless of SQLite's tie-break
// on `ORDER BY created_at` (which ties at one-second granularity).
export function byOrdinal(rows: Row[]): Row[] {
  const ordinal = (row: Row): number => Number(String(row.id).split('_').pop())
  return [...rows].sort((a, b) => ordinal(a) - ordinal(b))
}

// ── state projections (full-ish rows, timestamps normalized) ──
// task_title/display_name are presentation strings derived by a separate module
// (orchestration-task-display); excluded here — the store cutover ports that
// derivation at swap time, not in this invariant harness.
export function projectState(
  map: Map<string, string>,
  tables: Record<string, Row[]>
): Record<string, Row[]> {
  return {
    messages: tables.messages.map((r) => ({
      id: canonId(map, r.id),
      from_handle: r.from_handle,
      to_handle: r.to_handle,
      subject: r.subject,
      body: r.body,
      type: r.type,
      priority: r.priority,
      thread_id: r.thread_id ?? null,
      payload: r.payload ?? null,
      read: r.read,
      sequence: r.sequence,
      delivered_at: normTs(r.delivered_at),
      created_at: normTs(r.created_at)
    })),
    tasks: tables.tasks.map((r) => ({
      id: canonId(map, r.id),
      parent_id: canonId(map, r.parent_id),
      created_by_terminal_handle: r.created_by_terminal_handle ?? null,
      spec: r.spec,
      status: r.status,
      deps: canonDeps(map, r.deps),
      result: r.result ?? null,
      completed_at: normTs(r.completed_at),
      created_at: normTs(r.created_at)
    })),
    dispatch_contexts: tables.dispatch_contexts.map((r) => ({
      id: canonId(map, r.id),
      task_id: canonId(map, r.task_id),
      assignee_handle: r.assignee_handle ?? null,
      status: r.status,
      failure_count: r.failure_count,
      last_failure: r.last_failure ?? null,
      dispatched_at: normTs(r.dispatched_at),
      completed_at: normTs(r.completed_at),
      created_at: normTs(r.created_at),
      last_heartbeat_at: normTs(r.last_heartbeat_at)
    })),
    decision_gates: tables.decision_gates.map((r) => ({
      id: canonId(map, r.id),
      task_id: canonId(map, r.task_id),
      question: r.question,
      options: r.options,
      status: r.status,
      resolution: r.resolution ?? null,
      resolved_at: normTs(r.resolved_at),
      created_at: normTs(r.created_at)
    })),
    coordinator_runs: tables.coordinator_runs.map((r) => ({
      id: canonId(map, r.id),
      spec: r.spec,
      status: r.status,
      coordinator_handle: r.coordinator_handle,
      poll_interval_ms: r.poll_interval_ms,
      completed_at: normTs(r.completed_at),
      created_at: normTs(r.created_at)
    }))
  }
}

// ── read projections (mirror the Rust typed structs; ids canonicalized) ──

export const readMessage = (map: Map<string, string>, r: Row): Row => ({
  id: canonId(map, r.id),
  from_handle: r.from_handle,
  to_handle: r.to_handle,
  subject: r.subject,
  body: r.body,
  type: r.type,
  priority: r.priority
})
export const readTask = (map: Map<string, string>, r: Row): Row => ({
  id: canonId(map, r.id),
  parent_id: canonId(map, r.parent_id),
  spec: r.spec,
  status: r.status,
  deps: canonDeps(map, r.deps),
  result: r.result ?? null
})
export const readGate = (map: Map<string, string>, r: Row): Row => ({
  id: canonId(map, r.id),
  task_id: canonId(map, r.task_id),
  question: r.question,
  options: r.options,
  status: r.status,
  resolution: r.resolution ?? null
})
export const readDispatch = (map: Map<string, string>, r: Row): Row => ({
  id: canonId(map, r.id),
  task_id: canonId(map, r.task_id),
  assignee_handle: r.assignee_handle ?? null,
  status: r.status,
  failure_count: r.failure_count
})
export const readRun = (map: Map<string, string>, r: Row): Row => ({
  id: canonId(map, r.id),
  spec: r.spec,
  status: r.status,
  coordinator_handle: r.coordinator_handle,
  poll_interval_ms: r.poll_interval_ms
})
