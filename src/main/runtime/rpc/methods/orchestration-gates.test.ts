import { afterEach, describe, expect, it, vi } from 'vitest'
import { OrchestrationDb } from '../../orchestration/db'
import { ORCHESTRATION_GATE_METHODS } from './orchestration-gates'

type MockCoordinatorInstance = {
  resolveRun?: () => void
  stopped: boolean
}

const coordinatorMock = vi.hoisted(() => {
  const instances: MockCoordinatorInstance[] = []

  class Coordinator {
    resolveRun?: () => void
    stopped = false

    constructor() {
      instances.push(this)
    }

    runFromExistingRun(): Promise<unknown> {
      return new Promise((resolve) => {
        this.resolveRun = () => {
          resolve({})
        }
      })
    }

    stop(): void {
      this.stopped = true
      this.resolveRun?.()
    }
  }

  return { Coordinator, instances }
})

vi.mock('../../orchestration/coordinator', () => ({
  Coordinator: coordinatorMock.Coordinator
}))

describe('orchestration gate RPC lifecycle recovery', () => {
  let db: OrchestrationDb | undefined

  afterEach(async () => {
    for (const instance of coordinatorMock.instances) {
      instance.resolveRun?.()
    }
    coordinatorMock.instances.length = 0
    // Why: flush microtasks so the handler's `.finally()` callback can clear
    // the module-scoped active coordinator before the next test starts.
    await Promise.resolve()
    await Promise.resolve()
    db?.close()
    db = undefined
  })

  function findMethod(name: string) {
    const method = ORCHESTRATION_GATE_METHODS.find((candidate) => candidate.name === name)
    if (!method) {
      throw new Error(`Method not found: ${name}`)
    }
    return method
  }

  async function call(name: string, params: Record<string, unknown>): Promise<unknown> {
    const method = findMethod(name)
    const parsed = method.params ? method.params.parse(params) : undefined
    return method.handler(parsed, {
      runtime: { getOrchestrationDb: () => db }
    } as never)
  }

  it('marks a stale durable run failed when stop has no live coordinator', async () => {
    db = new OrchestrationDb(':memory:')
    const staleRun = db.createCoordinatorRun({
      spec: 'old work',
      coordinatorHandle: 'coordinator'
    })

    await expect(call('orchestration.runStop', {})).resolves.toEqual({
      runId: staleRun.id,
      stopped: true
    })

    expect(db.getCoordinatorRun(staleRun.id)?.status).toBe('failed')
    expect(db.getActiveCoordinatorRun()).toBeUndefined()
    expect(coordinatorMock.instances).toHaveLength(0)
  })

  it('fails a stale durable run before accepting a new coordinator run', async () => {
    db = new OrchestrationDb(':memory:')
    const staleRun = db.createCoordinatorRun({
      spec: 'old work',
      coordinatorHandle: 'coordinator'
    })

    const result = (await call('orchestration.run', { spec: 'new work' })) as {
      runId: string
      status: string
    }

    expect(result.status).toBe('running')
    expect(result.runId).not.toBe(staleRun.id)
    expect(db.getCoordinatorRun(staleRun.id)?.status).toBe('failed')
    expect(db.getActiveCoordinatorRun()?.id).toBe(result.runId)
    expect(coordinatorMock.instances).toHaveLength(1)
  })

  it('rejects a new run while a live coordinator is active', async () => {
    db = new OrchestrationDb(':memory:')

    const result = (await call('orchestration.run', { spec: 'active work' })) as {
      runId: string
      status: string
    }

    expect(result.status).toBe('running')
    expect(db.getActiveCoordinatorRun()?.id).toBe(result.runId)
    expect(coordinatorMock.instances).toHaveLength(1)

    await expect(call('orchestration.run', { spec: 'new work' })).rejects.toThrow(
      `Coordinator already running: ${result.runId}`
    )
    expect(db.getActiveCoordinatorRun()?.id).toBe(result.runId)
    expect(coordinatorMock.instances).toHaveLength(1)
  })

  it('allows coordinators with different handles to run concurrently', async () => {
    db = new OrchestrationDb(':memory:')

    const first = (await call('orchestration.run', { spec: 'a', from: 'coord-a' })) as {
      runId: string
    }
    const second = (await call('orchestration.run', { spec: 'b', from: 'coord-b' })) as {
      runId: string
    }

    expect(first.runId).not.toBe(second.runId)
    expect(
      db
        .getActiveCoordinatorRuns()
        .map((run) => run.id)
        .sort()
    ).toEqual([first.runId, second.runId].sort())
    expect(coordinatorMock.instances).toHaveLength(2)

    await expect(call('orchestration.run', { spec: 'c', from: 'coord-a' })).rejects.toThrow(
      `Coordinator already running: ${first.runId}`
    )
    expect(coordinatorMock.instances).toHaveLength(2)
  })

  it('stops only the targeted run and leaves the other coordinator alive', async () => {
    db = new OrchestrationDb(':memory:')

    const first = (await call('orchestration.run', { spec: 'a', from: 'coord-a' })) as {
      runId: string
    }
    const second = (await call('orchestration.run', { spec: 'b', from: 'coord-b' })) as {
      runId: string
    }

    // Why: with several orchestrators live an untargeted stop would pick one
    // arbitrarily — the mutual-kill in #4389 — so it must demand a target.
    await expect(call('orchestration.runStop', {})).rejects.toThrow(
      'Multiple active coordinator runs'
    )

    await expect(call('orchestration.runStop', { from: 'coord-a' })).resolves.toEqual({
      runId: first.runId,
      stopped: true
    })
    expect(coordinatorMock.instances[0].stopped).toBe(true)
    expect(coordinatorMock.instances[1].stopped).toBe(false)

    await expect(call('orchestration.runStop', { runId: second.runId })).resolves.toEqual({
      runId: second.runId,
      stopped: true
    })
    expect(coordinatorMock.instances[1].stopped).toBe(true)
  })

  it('reaps only the same-handle stale row when starting a new run', async () => {
    db = new OrchestrationDb(':memory:')
    const staleA = db.createCoordinatorRun({ spec: 'old a', coordinatorHandle: 'coord-a' })
    const staleB = db.createCoordinatorRun({ spec: 'old b', coordinatorHandle: 'coord-b' })

    const result = (await call('orchestration.run', { spec: 'new a', from: 'coord-a' })) as {
      runId: string
    }

    expect(result.runId).not.toBe(staleA.id)
    expect(db.getCoordinatorRun(staleA.id)?.status).toBe('failed')
    expect(db.getCoordinatorRun(staleB.id)?.status).toBe('running')
  })

  it('rejects a targeted stop for an unknown run or handle', async () => {
    db = new OrchestrationDb(':memory:')

    await expect(call('orchestration.runStop', { runId: 'run_missing' })).rejects.toThrow(
      'No active coordinator run: run_missing'
    )
    await expect(call('orchestration.runStop', { from: 'coord-missing' })).rejects.toThrow(
      'No active coordinator run for handle: coord-missing'
    )
  })
})
