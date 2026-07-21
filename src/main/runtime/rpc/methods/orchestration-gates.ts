import { z } from 'zod'
import { defineMethod, type RpcMethod } from '../core'
import { OptionalFiniteNumber, OptionalString, requiredString } from '../schemas'
import type { CoordinatorRun, GateStatus, OrchestrationDb } from '../../orchestration/db'
import { Coordinator } from '../../orchestration/coordinator'

// Why: live coordinators are keyed by run id so orchestration.runStop can
// target one without touching the others. Runs are keyed by coordinator
// handle (#4389): one live run per handle, but different handles may
// coordinate concurrently in the same workspace.
const activeCoordinators = new Map<string, { coordinator: Coordinator; handle: string }>()

function findLiveRunIdForHandle(handle: string): string | undefined {
  for (const [runId, entry] of activeCoordinators) {
    if (entry.handle === handle) {
      return runId
    }
  }
  return undefined
}

function markStaleCoordinatorRunFailed(db: OrchestrationDb, run: CoordinatorRun): void {
  // Why: a process restart loses the in-memory coordinator handle but can
  // leave the durable row marked running. Without a live handle, the row cannot
  // make progress, so fail it before accepting or acknowledging new lifecycle
  // commands.
  db.updateCoordinatorRun(run.id, 'failed')
}

const RunParams = z.object({
  spec: requiredString('Missing --spec'),
  from: OptionalString,
  pollIntervalMs: OptionalFiniteNumber,
  maxConcurrent: OptionalFiniteNumber,
  worktree: OptionalString
})

const RunStopParams = z.object({
  runId: OptionalString,
  from: OptionalString
})

const GateCreateParams = z.object({
  task: requiredString('Missing --task'),
  question: requiredString('Missing --question'),
  options: OptionalString
})

const GateResolveParams = z.object({
  id: requiredString('Missing --id'),
  resolution: requiredString('Missing --resolution')
})

const GateListParams = z.object({
  task: OptionalString,
  status: z.enum(['pending', 'resolved', 'timeout']).optional()
})

export const ORCHESTRATION_GATE_METHODS: RpcMethod[] = [
  // Why: Section 4.12 — orchestration.run returns immediately with a run ID.
  // The coordinator loop runs in the background; progress is queried via
  // orchestration.taskList. This prevents the RPC call from blocking the
  // CLI (or any caller) for the entire duration of the pipeline.
  defineMethod({
    name: 'orchestration.run',
    params: RunParams,
    handler: (params, { runtime }) => {
      const db = runtime.getOrchestrationDb()

      const coordinatorHandle = params.from ?? 'coordinator'
      const liveRunId = findLiveRunIdForHandle(coordinatorHandle)
      if (liveRunId) {
        throw new Error(`Coordinator already running: ${liveRunId}`)
      }
      // Why: only rows owned by THIS handle gate or get reaped here — another
      // handle's live run must neither block this start nor be failed as stale.
      for (const existing of db.getActiveCoordinatorRuns()) {
        if (existing.coordinator_handle !== coordinatorHandle) {
          continue
        }
        if (activeCoordinators.has(existing.id)) {
          throw new Error(`Coordinator already running: ${existing.id}`)
        }
        markStaleCoordinatorRunFailed(db, existing)
      }

      const coordinator = new Coordinator(db, runtime, {
        spec: params.spec,
        coordinatorHandle,
        pollIntervalMs: params.pollIntervalMs,
        maxConcurrent: params.maxConcurrent,
        worktree: params.worktree
      })

      const run = db.createCoordinatorRun({
        spec: params.spec,
        coordinatorHandle,
        pollIntervalMs: params.pollIntervalMs
      })

      activeCoordinators.set(run.id, { coordinator, handle: coordinatorHandle })

      // Why: fire-and-forget — the coordinator loop runs in the event loop
      // background. Results are persisted to the DB; callers query via
      // orchestration.taskList or orchestration.runStatus.
      coordinator.runFromExistingRun(run.id).finally(() => {
        if (activeCoordinators.get(run.id)?.coordinator === coordinator) {
          activeCoordinators.delete(run.id)
        }
      })

      return { runId: run.id, status: 'running' }
    }
  }),

  defineMethod({
    name: 'orchestration.runStop',
    params: RunStopParams,
    handler: (params, { runtime }) => {
      const db = runtime.getOrchestrationDb()
      const activeRuns = db.getActiveCoordinatorRuns()

      let run: CoordinatorRun
      if (params.runId) {
        const match = activeRuns.find((candidate) => candidate.id === params.runId)
        if (!match) {
          throw new Error(`No active coordinator run: ${params.runId}`)
        }
        run = match
      } else if (params.from) {
        const match = activeRuns.find(
          (candidate) => candidate.coordinator_handle === params.from
        )
        if (!match) {
          throw new Error(`No active coordinator run for handle: ${params.from}`)
        }
        run = match
      } else {
        if (activeRuns.length === 0) {
          throw new Error('No active coordinator run')
        }
        // Why: an untargeted stop with several orchestrators live would pick one
        // arbitrarily — the mutual-kill in #4389 — so demand a target instead.
        if (activeRuns.length > 1) {
          throw new Error(
            `Multiple active coordinator runs (${activeRuns
              .map((candidate) => `${candidate.id}:${candidate.coordinator_handle}`)
              .join(', ')}); pass --run <run_id> or --from <handle>`
          )
        }
        run = activeRuns[0]
      }

      const live = activeCoordinators.get(run.id)
      if (live) {
        live.coordinator.stop()
      } else {
        markStaleCoordinatorRunFailed(db, run)
      }

      return { runId: run.id, stopped: true }
    }
  }),

  defineMethod({
    name: 'orchestration.gateCreate',
    params: GateCreateParams,
    handler: (params, { runtime }) => {
      const db = runtime.getOrchestrationDb()
      let options: string[] | undefined
      if (params.options) {
        try {
          const parsed = JSON.parse(params.options)
          if (!Array.isArray(parsed) || !parsed.every((option) => typeof option === 'string')) {
            throw new Error('not an array of strings')
          }
          options = parsed
        } catch {
          throw new Error('Invalid --options: must be a JSON array of strings')
        }
      }
      const gate = db.createGate({
        taskId: params.task,
        question: params.question,
        options
      })
      return { gate }
    }
  }),

  defineMethod({
    name: 'orchestration.gateResolve',
    params: GateResolveParams,
    handler: (params, { runtime }) => {
      const db = runtime.getOrchestrationDb()
      const gate = db.resolveGate(params.id, params.resolution)
      if (!gate) {
        throw new Error(`Gate not found: ${params.id}`)
      }
      return { gate }
    }
  }),

  defineMethod({
    name: 'orchestration.gateList',
    params: GateListParams,
    handler: (params, { runtime }) => {
      const db = runtime.getOrchestrationDb()
      const gates = db.listGates({
        taskId: params.task,
        status: params.status as GateStatus
      })
      return { gates, count: gates.length }
    }
  })
]
