import type { AppState } from '../types'
import type { TuiAgent } from '../../../../shared/types'
import { callRuntimeRpc, RuntimeRpcCallError } from '@/runtime/runtime-rpc-client'

// Why: module-scoped (not in the store) so concurrent refreshes for one
// environment share a single in-flight promise without storing a Promise in
// Zustand state — matches the ensure*DetectedAgents dedup maps.
const runtimeRefreshPromises = new Map<string, Promise<TuiAgent[]>>()

export function _getRuntimeRefreshPromiseCountForTest(): number {
  return runtimeRefreshPromises.size
}

type RuntimeAgentRefreshDeps = {
  get: () => AppState
  set: (partial: Partial<AppState> | ((state: AppState) => Partial<AppState>)) => void
}

async function refreshOrDetectRuntimeAgents(environmentId: string): Promise<TuiAgent[]> {
  const target = { kind: 'environment', environmentId } as const
  try {
    const result = await callRuntimeRpc<{ agents: TuiAgent[] }>(target, 'preflight.refreshAgents')
    return result.agents
  } catch (error) {
    // Why (#9790): servers predating the refresh RPC reject with an error
    // envelope; fall back to a plain detect so refresh still re-probes there.
    // A transport/connection failure is not an RPC error envelope, so it
    // propagates to the caller and keeps the last-known list instead.
    if (!(error instanceof RuntimeRpcCallError)) {
      throw error
    }
    return callRuntimeRpc<TuiAgent[]>(target, 'preflight.detectAgents')
  }
}

/**
 * Re-detect agents on the owning runtime host (`preflight.refreshAgents`,
 * falling back to `preflight.detectAgents` on older servers). Keeps the
 * last-known list when the runtime is unreachable so a transient disconnect
 * does not blank the launch surface (#9790). Concurrent callers for the same
 * environment share one pending promise.
 */
export function runRuntimeAgentRefresh(
  { get, set }: RuntimeAgentRefreshDeps,
  environmentId: string
): Promise<TuiAgent[]> {
  const inflight = runtimeRefreshPromises.get(environmentId)
  if (inflight) {
    return inflight
  }

  set((s) => ({
    isRefreshingRuntimeAgents: { ...s.isRefreshingRuntimeAgents, [environmentId]: true }
  }))

  const pending = refreshOrDetectRuntimeAgents(environmentId)
    .then((agents) => {
      set((s) => ({
        runtimeDetectedAgentIds: { ...s.runtimeDetectedAgentIds, [environmentId]: agents },
        isRefreshingRuntimeAgents: { ...s.isRefreshingRuntimeAgents, [environmentId]: false }
      }))
      return agents
    })
    .catch(() => {
      const fallback = get().runtimeDetectedAgentIds[environmentId] ?? []
      set((s) => ({
        isRefreshingRuntimeAgents: { ...s.isRefreshingRuntimeAgents, [environmentId]: false }
      }))
      return fallback
    })
    .finally(() => {
      if (runtimeRefreshPromises.get(environmentId) === pending) {
        runtimeRefreshPromises.delete(environmentId)
      }
    })

  runtimeRefreshPromises.set(environmentId, pending)
  return pending
}
