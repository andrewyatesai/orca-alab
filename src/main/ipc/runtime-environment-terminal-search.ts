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
import type {
  RemoteTerminalSearchContextResult,
  RemoteTerminalSearchResult
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

function isSearchResultShape(value: unknown): value is RemoteTerminalSearchResult {
  if (!value || typeof value !== 'object') {
    return false
  }
  const result = value as Partial<RemoteTerminalSearchResult>
  return (
    typeof result.searchSchema === 'number' &&
    result.searchSchema >= 1 &&
    typeof result.available === 'boolean' &&
    Array.isArray(result.matches) &&
    typeof result.total === 'number' &&
    typeof result.incomplete === 'boolean'
  )
}

function isSearchContextShape(value: unknown): value is RemoteTerminalSearchContextResult {
  if (!value || typeof value !== 'object') {
    return false
  }
  const result = value as Partial<RemoteTerminalSearchContextResult>
  return (
    typeof result.searchSchema === 'number' &&
    result.searchSchema >= 1 &&
    typeof result.available === 'boolean' &&
    Array.isArray(result.lines)
  )
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
    response = await callEnvironment(
      userDataPath,
      environmentId,
      method,
      params,
      REMOTE_SEARCH_TIMEOUT_MS
    )
  } catch {
    return { kind: 'unavailable', reason: 'unreachable' }
  }
  // Why after the await: an abort during flight (Esc / generation bump) must
  // not surface stale results into the palette.
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
    (value) => (isSearchResultShape(value) ? value : null),
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
    (value) => (isSearchContextShape(value) ? value : null),
    opts.signal
  )
  if (outcome.kind !== 'ok') {
    return outcome
  }
  return outcome.result.available
    ? { kind: 'context', result: outcome.result }
    : { kind: 'unavailable', reason: 'pane-unsearchable' }
}
