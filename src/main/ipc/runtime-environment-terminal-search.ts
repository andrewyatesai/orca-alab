// Fed §2.4: client side of the remote terminal-search wire. Routes
// `terminal.search` / `terminal.searchContext` to a runtime environment over
// whatever transport the environment already uses (direct WS, cached request
// connection, shared control channel — provider-generic, nothing
// GitHub/GitLab-specific), and turns OLD-HOST failures into a per-pane
// "source unavailable" verdict instead of failing the federated query.
//
// Old-host detection is capability-cached PER ENVIRONMENT (the
// GitCapabilityCache pattern: narrow unsupported-error predicate, no repeated
// probing) so one `method_not_found` degrades every pane on that host without
// re-asking, while other hosts keep searching.

import type { RuntimeRpcResponse } from '../../shared/runtime-rpc-envelope'
import {
  isRemoteTerminalSearchContextResultShape,
  isRemoteTerminalSearchResultShape,
  type RemoteTerminalSearchContextResult,
  type RemoteTerminalSearchResult
} from '../../shared/terminal-remote-search-protocol'
import { callRuntimeEnvironment } from './runtime-environment-transport-routing'

export type RemoteTerminalSearchRequest = {
  terminal: string
  query: string
  caseSensitive?: boolean
  regex?: boolean
  maxMatches?: number
  gen?: number
  clientAnchorGen?: number
}

export type RemoteTerminalSearchOutcome =
  | { kind: 'results'; result: RemoteTerminalSearchResult }
  /** The pane's source is unavailable — controller shows it as such, never as
   *  an empty or failed result set. */
  | { kind: 'unavailable'; reason: 'unsupported-host' | 'pane-unsearchable' | 'unreachable' }

export type RemoteTerminalSearchContextOutcome =
  | { kind: 'context'; result: RemoteTerminalSearchContextResult }
  | { kind: 'unavailable'; reason: 'unsupported-host' | 'pane-unsearchable' | 'unreachable' }

// Why not time-bounded: a host only gains the method by upgrading, which
// re-pairs/reconnects; the reconnect hooks below clear the verdict.
const unsupportedSearchEnvironments = new Set<string>()

export function clearRemoteTerminalSearchSupport(environmentId: string): void {
  unsupportedSearchEnvironments.delete(environmentId)
}

export function resetRemoteTerminalSearchSupportForTest(): void {
  unsupportedSearchEnvironments.clear()
}

/** Narrow predicate (git-compatibility rule): ONLY the dispatcher's
 *  unknown-method verdict marks a host unsupported — transport failures and
 *  handler errors must not poison the capability cache. */
function isMethodNotFound(response: RuntimeRpcResponse<unknown>): boolean {
  return response.ok === false && response.error.code === 'method_not_found'
}

type RuntimeEnvironmentCall = (
  userDataPath: string,
  selector: string,
  method: string,
  params: unknown,
  timeoutMs?: number
) => Promise<RuntimeRpcResponse<unknown>>

// Why the seam: tests (and the relay-less in-process path) inject a fake
// transport; production uses the real environment routing.
let callEnvironment: RuntimeEnvironmentCall = callRuntimeEnvironment

export function setRuntimeEnvironmentTerminalSearchTransportForTest(
  call: RuntimeEnvironmentCall | null
): void {
  callEnvironment = call ?? callRuntimeEnvironment
}

// Why: remote sources stream in behind local results (fed §1 liveness); a
// hung host must release the palette's per-source slot well before the
// runtime default 15s.
const REMOTE_SEARCH_TIMEOUT_MS = 8_000

/** Resolve with the transport response, or reject the moment `signal` aborts —
 *  the pending transport promise is left to settle and be ignored. */
function raceTransportAgainstAbort<T>(pending: Promise<T>, signal?: AbortSignal): Promise<T> {
  if (!signal) {
    return pending
  }
  return new Promise<T>((resolve, reject) => {
    const onAbort = (): void => reject(new Error('remote search aborted'))
    signal.addEventListener('abort', onAbort, { once: true })
    pending
      .then(resolve, reject)
      .finally(() => signal.removeEventListener('abort', onAbort))
      // Why: when the abort already rejected this race, the transport's own
      // later rejection has no listener — swallow it so it can't surface as
      // an unhandled rejection.
      .catch(() => undefined)
  })
}

async function callWithDegradation<T>(
  userDataPath: string,
  environmentId: string,
  method: 'terminal.search' | 'terminal.searchContext',
  params: unknown,
  parse: (value: unknown) => T | null,
  signal?: AbortSignal
): Promise<
  | { kind: 'ok'; result: T }
  | { kind: 'unavailable'; reason: 'unsupported-host' | 'pane-unsearchable' | 'unreachable' }
> {
  if (unsupportedSearchEnvironments.has(environmentId)) {
    return { kind: 'unavailable', reason: 'unsupported-host' }
  }
  if (signal?.aborted) {
    return { kind: 'unavailable', reason: 'unreachable' }
  }
  let response: RuntimeRpcResponse<unknown>
  try {
    // The abort races the transport await so a cancel (Esc / generation bump)
    // releases the palette's per-source slot IMMEDIATELY. The runtime-envelope
    // protocol has no in-flight cancel message (unlike the SSH relay's
    // rpc.cancel, which the mux route uses) — the host finishes its bounded
    // scan and the late response is dropped; adding a protocol cancel is
    // recorded follow-up, not silently claimed here.
    response = await raceTransportAgainstAbort(
      callEnvironment(userDataPath, environmentId, method, params, REMOTE_SEARCH_TIMEOUT_MS),
      signal
    )
  } catch {
    return { kind: 'unavailable', reason: 'unreachable' }
  }
  // Why after the await: an abort during flight must never surface stale
  // results into the palette even when the response won the race.
  if (signal?.aborted) {
    return { kind: 'unavailable', reason: 'unreachable' }
  }
  if (isMethodNotFound(response)) {
    unsupportedSearchEnvironments.add(environmentId)
    return { kind: 'unavailable', reason: 'unsupported-host' }
  }
  if (response.ok !== true) {
    return { kind: 'unavailable', reason: 'unreachable' }
  }
  const parsed = parse(response.result)
  if (!parsed) {
    // A host that answers with an unrecognizable shape is as unusable as one
    // without the method — but do NOT cache: it may be a transient envelope bug.
    return { kind: 'unavailable', reason: 'pane-unsearchable' }
  }
  return { kind: 'ok', result: parsed }
}

export async function searchRuntimeEnvironmentTerminal(
  userDataPath: string,
  environmentId: string,
  request: RemoteTerminalSearchRequest,
  opts: { signal?: AbortSignal } = {}
): Promise<RemoteTerminalSearchOutcome> {
  const outcome = await callWithDegradation(
    userDataPath,
    environmentId,
    'terminal.search',
    request,
    (value) => (isRemoteTerminalSearchResultShape(value) ? value : null),
    opts.signal
  )
  if (outcome.kind !== 'ok') {
    return outcome
  }
  return outcome.result.available
    ? { kind: 'results', result: outcome.result }
    : { kind: 'unavailable', reason: 'pane-unsearchable' }
}

export async function runtimeEnvironmentTerminalSearchContext(
  userDataPath: string,
  environmentId: string,
  request: { terminal: string; hostRow: number; before?: number; after?: number },
  opts: { signal?: AbortSignal } = {}
): Promise<RemoteTerminalSearchContextOutcome> {
  const outcome = await callWithDegradation(
    userDataPath,
    environmentId,
    'terminal.searchContext',
    request,
    (value) => (isRemoteTerminalSearchContextResultShape(value) ? value : null),
    opts.signal
  )
  if (outcome.kind !== 'ok') {
    return outcome
  }
  return outcome.result.available
    ? { kind: 'context', result: outcome.result }
    : { kind: 'unavailable', reason: 'pane-unsearchable' }
}
