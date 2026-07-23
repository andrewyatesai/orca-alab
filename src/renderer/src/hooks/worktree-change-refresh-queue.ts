import type { ExecutionHostId } from '../../../shared/execution-host'

type WorktreeRename = {
  oldWorktreeId: string
  newWorktreeId: string
}

type WorktreeChangeEvent = {
  repoId: string
  ownerHostId?: ExecutionHostId
  renamed?: WorktreeRename
  // Why: set on a local worktrees:changed while a remote runtime is active, so the
  // refresh pins to the local host instead of dropping the event (see useIpcEvents).
  forceLocalOwner?: boolean
}

type WorktreeChangeRefreshHandler = (
  repoId: string,
  ownerHostId?: ExecutionHostId,
  renamed?: WorktreeRename,
  options?: { forceLocalOwner?: boolean }
) => Promise<void>

type QueuedWorktreeChange = {
  renamed?: WorktreeRename
  forceLocalOwner?: boolean
}

type RepoRefreshState = {
  repoId: string
  ownerHostId?: ExecutionHostId
  running: boolean
  queue: QueuedWorktreeChange[]
}

export type WorktreeChangeRefreshQueue = {
  dispose: () => void
  enqueue: (event: WorktreeChangeEvent) => void
}

export function createWorktreeChangeRefreshQueue(
  handler: WorktreeChangeRefreshHandler
): WorktreeChangeRefreshQueue {
  const states = new Map<string, RepoRefreshState>()
  let disposed = false

  // Why: a repo id can exist on several hosts; serialize refreshes per host so one host's scan doesn't starve another's.
  const getRefreshKey = (event: Pick<WorktreeChangeEvent, 'repoId' | 'ownerHostId'>) =>
    `${event.ownerHostId ?? 'focused'}\0${event.repoId}`

  const drain = async (key: string, state: RepoRefreshState): Promise<void> => {
    state.running = true
    try {
      while (!disposed && state.queue.length > 0) {
        const next = state.queue.shift()
        try {
          await handler(state.repoId, state.ownerHostId, next?.renamed, {
            forceLocalOwner: next?.forceLocalOwner
          })
        } catch (error) {
          console.error('Failed to refresh changed worktrees:', error)
        }
      }
    } finally {
      state.running = false
      if (disposed || state.queue.length === 0) {
        states.delete(key)
      } else {
        void drain(key, state)
      }
    }
  }

  return {
    dispose() {
      disposed = true
      states.clear()
    },

    enqueue(event) {
      if (disposed) {
        return
      }
      const key = getRefreshKey(event)
      let state = states.get(key)
      if (!state) {
        state = { repoId: event.repoId, ownerHostId: event.ownerHostId, running: false, queue: [] }
        states.set(key, state)
      }

      if (event.renamed) {
        state.queue.push({ renamed: event.renamed, forceLocalOwner: event.forceLocalOwner })
      } else {
        const lastQueued = state.queue.at(-1)
        // Why: Windows/OneDrive can emit a burst for one checkout change. Keep a
        // trailing refresh, but do not fan out adjacent identical repo scans.
        // A differing forceLocalOwner is not identical — keep it as its own scan so
        // a local-pinned refresh is never coalesced into a runtime-routed one.
        if (
          !lastQueued ||
          lastQueued.renamed !== undefined ||
          Boolean(lastQueued.forceLocalOwner) !== Boolean(event.forceLocalOwner)
        ) {
          state.queue.push({ forceLocalOwner: event.forceLocalOwner })
        }
      }

      if (!state.running) {
        void drain(key, state)
      }
    }
  }
}
