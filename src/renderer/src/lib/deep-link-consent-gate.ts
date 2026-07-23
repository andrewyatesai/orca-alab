import type { OrcaDeepLink, OrcaDeepLinkOrigin } from '../../../shared/orca-deep-link'

/** Pending `orca://run` consent (#4384 §6.1). There is exactly one slot, no
 *  queue and no "always allow": every run link needs its own explicit confirm. */
export type DeepLinkRunConsentRequest = {
  link: Extract<OrcaDeepLink, { kind: 'run' }>
  origin: OrcaDeepLinkOrigin
}

// Why: mirrors the OS router's queue depth (§6.3) — a link flood while consent is open must stay bounded.
const MAX_DEFERRED_NAVIGATIONS = 4

let pendingRequest: DeepLinkRunConsentRequest | null = null
let deferredNavigations: (() => void)[] = []
const listeners = new Set<() => void>()

function notifyListeners(): void {
  // Why: snapshot — a notified subscriber may re-subscribe (React store semantics) mid-iteration.
  for (const listener of Array.from(listeners)) {
    listener()
  }
}

export function getPendingRunCommandConsent(): DeepLinkRunConsentRequest | null {
  return pendingRequest
}

/** Subscribe for `useSyncExternalStore`; fires on every pending-request change. */
export function subscribeRunCommandConsent(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

/** Returns false (request dropped) while another consent is open — swapping the
 *  dialog's content under the user's pointer would let a second link steal the
 *  confirmation aimed at the first (§6.1). */
export function requestRunCommandConsent(request: DeepLinkRunConsentRequest): boolean {
  if (pendingRequest !== null) {
    return false
  }
  pendingRequest = request
  notifyListeners()
  return true
}

/** Close the consent dialog (confirm or cancel) and release held navigation. */
export function settleRunCommandConsent(): void {
  if (pendingRequest === null) {
    return
  }
  pendingRequest = null
  // Why: drain AFTER clearing so a navigation can't observe a half-open dialog.
  const queued = deferredNavigations
  deferredNavigations = []
  notifyListeners()
  for (const navigate of queued) {
    navigate()
  }
}

/** Navigation dispatches (ui:focusTerminal / ui:activateWorktree / in-pane
 *  worktree links) run immediately, unless a consent dialog is open — then they
 *  are held until it closes so a focus link can't re-target the UI mid-consent
 *  (clickjack hardening, #4384 §6.3). */
export function runOrDeferDeepLinkNavigation(navigate: () => void): void {
  if (pendingRequest === null) {
    navigate()
    return
  }
  if (deferredNavigations.length >= MAX_DEFERRED_NAVIGATIONS) {
    deferredNavigations.shift()
  }
  deferredNavigations.push(navigate)
}

export function resetRunCommandConsentForTest(): void {
  pendingRequest = null
  deferredNavigations = []
  listeners.clear()
}
