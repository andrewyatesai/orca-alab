import { toRuntimeExecutionHostId } from '../shared/execution-host'
import { markEnvironmentUsed } from '../shared/runtime-environment-store'

type PtyBindingStore = {
  clearHostWorkspaceSessionPtyBindings: (hostId?: string | null) => void
}

let registeredStore: PtyBindingStore | null = null

/** Register the persistence store the churn prune writes through. Called once at
 *  startup; transport routing cannot import the Store directly (it is shared with
 *  processes that have no persistence). */
export function registerRuntimeHostPtyBindingChurnPruneStore(store: PtyBindingStore): void {
  registeredStore = store
}

export function resetRuntimeHostPtyBindingChurnPruneStoreForTests(): void {
  registeredStore = null
}

/** Drop the persisted PTY-handle tab bindings for a runtime environment whose
 *  runtimeId churned (host restarted). Why: every `remote:<env>@@term_*` handle
 *  died with the old runtime instance; keeping the bindings makes the next
 *  restore reattach-fail and respawn terminals the user closed (#9352). */
export function pruneRuntimeHostPtyBindingsOnRuntimeChurn(environmentId: string): void {
  registeredStore?.clearHostWorkspaceSessionPtyBindings(toRuntimeExecutionHostId(environmentId))
}

/** Record a runtime round-trip and, when the observed runtimeId replaced a
 *  previously known one (host restart), prune that environment's dead handles. */
export function markEnvironmentUsedWithChurnPrune(
  userDataPath: string,
  environmentId: string,
  runtimeId: string
): void {
  const { runtimeInstanceChanged } = markEnvironmentUsed(userDataPath, environmentId, { runtimeId })
  if (runtimeInstanceChanged) {
    pruneRuntimeHostPtyBindingsOnRuntimeChurn(environmentId)
  }
}
