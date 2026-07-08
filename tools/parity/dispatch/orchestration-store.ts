// TS dispatch for the orchestration-store parity module — the state-replay half
// of the orca-runtime store cutover. The orchestration DB is a *stateful*
// SQLite store, so we make it fit the pure input→output parity model by
// treating "replay this operation sequence on a fresh in-memory store, then dump
// the canonical final state + read results" as a pure function. This adapter
// drives the live TypeScript `OrchestrationDb`; the Rust module drives the
// orca-runtime port over the same op-sequence vectors. Canonicalization (id
// remap + timestamp normalization) lives in ./orchestration-store-canonical.
//
// Ops reference prior-created entities by "@<opIndex>", resolved to the actual
// id at replay; the canonical output erases the store's random ids + SQL
// timestamps so TS and Rust are comparable despite each generating its own.
import { OrchestrationDb } from '../../../src/main/runtime/orchestration/db'
import type { RustOrchestrationStoreHandle } from '../../../src/main/daemon/rust-git-addon'
import {
  buildIdMap,
  byOrdinal,
  projectState,
  readDispatch,
  readGate,
  readMessage,
  readRun,
  readTask,
  type Row
} from './orchestration-store-canonical'

// The shim delegates to a `private` napi store; reach it for the raw all-tables
// dump (there is no public "list all dispatch contexts"). Contained to this
// test harness. The Rust `dumpTablesJson` serializes each full row to its TS Row
// shape, ORDER BY rowid — the same shape+order the old `SELECT *` dump produced.
function dumpRawTables(store: OrchestrationDb): Record<string, Row[]> {
  const handle = (store as unknown as { store: RustOrchestrationStoreHandle }).store
  return JSON.parse(handle.dumpTablesJson()) as Record<string, Row[]>
}

type Op = Record<string, unknown>
// A read op captures its raw rows now; they are canonicalized after replay once
// the final id map exists. `finalize(map)` produces the comparable value.
type PendingResult = { ok: boolean } | { finalize: (map: Map<string, string>) => unknown }

function resolveRef(ref: unknown, createdIds: (string | undefined)[]): string {
  if (typeof ref === 'string' && ref.startsWith('@')) {
    const id = createdIds[Number(ref.slice(1))]
    if (id === undefined) {
      throw new Error(`orchestration-store: unresolved ref ${ref}`)
    }
    return id
  }
  return String(ref)
}

function runOpSequence(input: { ops: Op[] }): unknown {
  const store = new OrchestrationDb(':memory:')
  try {
    const createdIds: (string | undefined)[] = []
    const pending: PendingResult[] = []
    const ref = (v: unknown): string => resolveRef(v, createdIds)

    for (const op of input.ops) {
      let created: string | undefined
      let result: PendingResult
      switch (op.op) {
        case 'sendMessage': {
          const row = store.insertMessage({
            from: op.from as string,
            to: op.to as string,
            subject: op.subject as string,
            body: op.body as string | undefined,
            type: op.type as never,
            priority: op.priority as never,
            threadId: op.threadId as string | undefined,
            payload: op.payload as string | undefined
          })
          created = row.id
          result = { ok: true }
          break
        }
        case 'markRead':
          store.markAsRead([ref(op.message)])
          result = { ok: true }
          break
        case 'markDelivered':
          store.markAsDelivered([ref(op.message)])
          result = { ok: true }
          break
        case 'createTask': {
          const row = store.createTask({
            spec: op.spec as string,
            deps: (op.deps as unknown[] | undefined)?.map(ref),
            parentId: op.parentId === undefined ? undefined : ref(op.parentId),
            createdByTerminalHandle: op.createdBy as string | undefined
          })
          created = row.id
          result = { ok: true }
          break
        }
        case 'updateTaskStatus': {
          const row = store.updateTaskStatus(ref(op.task), op.status as never, op.result as string | undefined)
          result = { ok: row !== undefined }
          break
        }
        case 'createDispatch': {
          try {
            const row = store.createDispatchContext(ref(op.task), op.assignee as string)
            created = row.id
            result = { ok: true }
          } catch {
            result = { ok: false }
          }
          break
        }
        case 'completeDispatch':
          store.completeDispatch(ref(op.dispatch))
          result = { ok: true }
          break
        case 'failDispatch': {
          const row = store.failDispatch(ref(op.dispatch), op.error as string)
          result = { ok: row !== undefined }
          break
        }
        case 'recordHeartbeat':
          store.recordHeartbeat(ref(op.dispatch), op.at as string)
          result = { ok: true }
          break
        case 'createGate': {
          const row = store.createGate({
            taskId: ref(op.task),
            question: op.question as string,
            options: op.options as string[] | undefined
          })
          created = row.id
          result = { ok: true }
          break
        }
        case 'resolveGate': {
          const row = store.resolveGate(ref(op.gate), op.resolution as string)
          result = { ok: row !== undefined }
          break
        }
        case 'timeoutGate': {
          const row = store.timeoutGate(ref(op.gate))
          result = { ok: row !== undefined }
          break
        }
        case 'createCoordinatorRun': {
          const row = store.createCoordinatorRun({
            spec: op.spec as string,
            coordinatorHandle: op.coordinator as string,
            pollIntervalMs: op.pollIntervalMs as number | undefined
          })
          created = row.id
          result = { ok: true }
          break
        }
        case 'updateCoordinatorRun': {
          const row = store.updateCoordinatorRun(ref(op.run), op.status as never)
          result = { ok: row !== undefined }
          break
        }
        case 'getUnreadMessages': {
          const rows = store.getUnreadMessages(op.handle as string)
          result = { finalize: (map) => byOrdinal(rows.map((r) => readMessage(map, r))) }
          break
        }
        case 'getUndeliveredUnread': {
          const rows = store.getUndeliveredUnreadMessages(op.handle as string)
          result = { finalize: (map) => byOrdinal(rows.map((r) => readMessage(map, r))) }
          break
        }
        case 'listTasks': {
          const rows = store.listTasks(op.status === undefined ? undefined : { status: op.status as never })
          result = { finalize: (map) => byOrdinal(rows.map((r) => readTask(map, r))) }
          break
        }
        case 'getTask': {
          const row = store.getTask(ref(op.task))
          result = { finalize: (map) => (row ? readTask(map, row) : null) }
          break
        }
        case 'listGates': {
          const rows = store.listGates({ taskId: ref(op.task), status: op.status as never })
          result = { finalize: (map) => byOrdinal(rows.map((r) => readGate(map, r))) }
          break
        }
        case 'getStaleDispatches': {
          const rows = store.getStaleDispatches(op.threshold as string)
          result = { finalize: (map) => byOrdinal(rows.map((r) => readDispatch(map, r))) }
          break
        }
        case 'getActiveCoordinatorRun': {
          const row = store.getActiveCoordinatorRun()
          result = { finalize: (map) => (row ? readRun(map, row) : null) }
          break
        }
        default:
          throw new Error(`orchestration-store: unknown op ${String(op.op)}`)
      }
      createdIds.push(created)
      pending.push(result)
    }

    const rawTables = dumpRawTables(store)
    const idMap = buildIdMap(rawTables)
    const ops = pending.map((entry) => ('finalize' in entry ? entry.finalize(idMap) : entry))
    return { ops, state: projectState(idMap, rawTables) }
  } finally {
    store.close()
  }
}

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'runOpSequence':
      return runOpSequence(input as { ops: Op[] })
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
