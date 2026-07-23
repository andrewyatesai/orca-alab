import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, describe, expect, it } from 'vitest'
import { OrchestrationDb } from './db'

// Pins #9694: OrchestrationDb.hasAnyDispatchContexts() is the cached emptiness
// probe buildAgentOrchestrationByPaneKey uses to skip its per-terminal dispatch
// fan-out on every 16ms graph publish for the never-orchestrate majority.
describe('OrchestrationDb.hasAnyDispatchContexts (empty-dispatch short-circuit)', () => {
  const tempDirs: string[] = []
  let openDbs: OrchestrationDb[] = []

  function memDb(): OrchestrationDb {
    const d = new OrchestrationDb(':memory:')
    openDbs.push(d)
    return d
  }

  function tempDbPath(): string {
    const dir = mkdtempSync(join(tmpdir(), 'orca-orch-empty-'))
    tempDirs.push(dir)
    return join(dir, 'orchestration.db')
  }

  function fileDb(path: string): OrchestrationDb {
    const d = new OrchestrationDb(path)
    openDbs.push(d)
    return d
  }

  afterEach(() => {
    for (const d of openDbs) {
      d.close()
    }
    openDbs = []
    for (const dir of tempDirs.splice(0)) {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  it('is false on a fresh, never-orchestrated DB', () => {
    expect(memDb().hasAnyDispatchContexts()).toBe(false)
  })

  it('flips true when a dispatch is created and stays true after it completes', () => {
    const d = memDb()
    const ctx = d.createDispatchContext(d.createTask({ spec: 'work' }).id, 'term_worker')
    expect(d.hasAnyDispatchContexts()).toBe(true)
    // Completed rows must still count so recent-completed lookups stay reachable.
    d.completeDispatch(ctx.id)
    expect(d.hasAnyDispatchContexts()).toBe(true)
  })

  it('resets clear the cache back to false', () => {
    const d = memDb()
    d.createDispatchContext(d.createTask({ spec: 'work' }).id, 'term_worker')
    expect(d.hasAnyDispatchContexts()).toBe(true)
    d.resetTasks()
    expect(d.hasAnyDispatchContexts()).toBe(false)

    d.createDispatchContext(d.createTask({ spec: 'work' }).id, 'term_worker')
    expect(d.hasAnyDispatchContexts()).toBe(true)
    d.resetAll()
    expect(d.hasAnyDispatchContexts()).toBe(false)
  })

  // The regression the naive listTasksWithDispatch derivation would cause: a
  // persisted *completed* dispatch has no active dispatch_id, so a cold reopen
  // must still report true or recent-completed context is silently dropped.
  it('cold-reopens true when a persisted dispatch has already completed', () => {
    const path = tempDbPath()
    const seed = fileDb(path)
    const ctx = seed.createDispatchContext(seed.createTask({ spec: 'work' }).id, 'term_worker')
    seed.completeDispatch(ctx.id)
    seed.close()
    openDbs = openDbs.filter((d) => d !== seed)

    // Fresh process view: cache is cold and derives from persisted state.
    expect(fileDb(path).hasAnyDispatchContexts()).toBe(true)
  })

  it('cold-reopens false when the persisted DB has no tasks or dispatches', () => {
    const path = tempDbPath()
    const seed = fileDb(path)
    seed.close()
    openDbs = openDbs.filter((d) => d !== seed)

    expect(fileDb(path).hasAnyDispatchContexts()).toBe(false)
  })
})
