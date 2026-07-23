// Fed §2.4: SSH-provider panes route `terminal.search`/`terminal.searchContext`
// over the channel multiplexer (5B "SSH mux routing"). The mux request carries a
// REAL in-flight abort: on AbortSignal the mux emits an `rpc.cancel` notification
// and the relay dispatcher aborts that request's controller (the relay abort
// path, src/relay/client-request-aborts.ts) — cancellation reaches the host, not
// just the client promise.
//
// Old-relay degradation mirrors the runtime-environment route: a relay without
// the method answers JSON-RPC -32601, which marks the HOST unsupported (cached
// per host key, cleared on re-deploy/reconnect) and the pane degrades to
// "source unavailable" — never a failed federated query. Provider-generic per
// AGENTS.md; schema validation shares the exact guards every other transport
// uses (terminal-remote-search-protocol).

import {
  isRemoteTerminalSearchContextResultShape,
  isRemoteTerminalSearchResultShape,
  type RemoteTerminalSearchContextResult,
  type RemoteTerminalSearchResult
} from '../../shared/terminal-remote-search-protocol'

/** The one mux capability this route needs (the real SshChannelMultiplexer's
 *  `request` — signal-aware, emits rpc.cancel on abort). */
export type SshRelaySearchMux = {
  request: (
    method: string,
    params?: Record<string, unknown>,
    options?: { signal?: AbortSignal; timeoutMs?: number }
  ) => Promise<unknown>
}

export type SshRelayTerminalSearchRequest = {
  terminal: string
  query: string
  caseSensitive?: boolean
  regex?: boolean
  maxMatches?: number
  gen?: number
  clientAnchorGen?: number
}

export type SshRelayTerminalSearchOutcome =
  | { kind: 'results'; result: RemoteTerminalSearchResult }
  | { kind: 'unavailable'; reason: 'unsupported-host' | 'pane-unsearchable' | 'unreachable' }

export type SshRelayTerminalSearchContextOutcome =
  | { kind: 'context'; result: RemoteTerminalSearchContextResult }
  | { kind: 'unavailable'; reason: 'unsupported-host' | 'pane-unsearchable' | 'unreachable' }

// Why: remote sources must release the palette slot well before the mux's own
// 30s default (fed §1 liveness — remote streams in behind local results).
const SSH_RELAY_SEARCH_TIMEOUT_MS = 8_000

// Why not time-bounded: a relay only gains the method via re-deploy, which
// reconnects; callers clear the verdict on reconnect/upgrade.
const unsupportedSearchHosts = new Set<string>()

export function clearSshRelayTerminalSearchSupport(hostKey: string): void {
  unsupportedSearchHosts.delete(hostKey)
}

export function resetSshRelayTerminalSearchSupportForTest(): void {
  unsupportedSearchHosts.clear()
}

function isMethodNotFoundError(error: unknown): boolean {
  // Narrow predicate (git-compatibility rule): ONLY the dispatcher's -32601
  // unknown-method error marks the host — transport failures must not poison
  // the capability cache.
  return error instanceof Error && (error as Error & { code?: unknown }).code === -32601
}

async function callRelayWithDegradation<T>(
  mux: SshRelaySearchMux,
  hostKey: string,
  method: 'terminal.search' | 'terminal.searchContext',
  params: Record<string, unknown>,
  parse: (value: unknown) => T | null,
  opts: { signal?: AbortSignal; timeoutMs?: number }
): Promise<
  | { kind: 'ok'; result: T }
  | { kind: 'unavailable'; reason: 'unsupported-host' | 'pane-unsearchable' | 'unreachable' }
> {
  if (unsupportedSearchHosts.has(hostKey)) {
    return { kind: 'unavailable', reason: 'unsupported-host' }
  }
  let raw: unknown
  try {
    raw = await mux.request(method, params, {
      signal: opts.signal,
      timeoutMs: opts.timeoutMs ?? SSH_RELAY_SEARCH_TIMEOUT_MS
    })
  } catch (error) {
    if (isMethodNotFoundError(error)) {
      unsupportedSearchHosts.add(hostKey)
      return { kind: 'unavailable', reason: 'unsupported-host' }
    }
    // Abort, timeout, channel death: this pane's source is unreachable for
    // this fan-out. The mux already sent rpc.cancel host-side on abort.
    return { kind: 'unavailable', reason: 'unreachable' }
  }
  if (opts.signal?.aborted) {
    // Response raced the abort: never surface stale results into the palette.
    return { kind: 'unavailable', reason: 'unreachable' }
  }
  const parsed = parse(raw)
  if (!parsed) {
    // Unrecognizable shape: unusable but NOT cached (may be transient skew).
    return { kind: 'unavailable', reason: 'pane-unsearchable' }
  }
  return { kind: 'ok', result: parsed }
}

export async function searchSshRelayTerminal(
  mux: SshRelaySearchMux,
  hostKey: string,
  request: SshRelayTerminalSearchRequest,
  opts: { signal?: AbortSignal; timeoutMs?: number } = {}
): Promise<SshRelayTerminalSearchOutcome> {
  const outcome = await callRelayWithDegradation(
    mux,
    hostKey,
    'terminal.search',
    { ...request },
    (value) => (isRemoteTerminalSearchResultShape(value) ? value : null),
    opts
  )
  if (outcome.kind !== 'ok') {
    return outcome
  }
  return outcome.result.available
    ? { kind: 'results', result: outcome.result }
    : { kind: 'unavailable', reason: 'pane-unsearchable' }
}

export async function sshRelayTerminalSearchContext(
  mux: SshRelaySearchMux,
  hostKey: string,
  request: { terminal: string; hostRow: number; before?: number; after?: number },
  opts: { signal?: AbortSignal; timeoutMs?: number } = {}
): Promise<SshRelayTerminalSearchContextOutcome> {
  const outcome = await callRelayWithDegradation(
    mux,
    hostKey,
    'terminal.searchContext',
    { ...request },
    (value) => (isRemoteTerminalSearchContextResultShape(value) ? value : null),
    opts
  )
  if (outcome.kind !== 'ok') {
    return outcome
  }
  return outcome.result.available
    ? { kind: 'context', result: outcome.result }
    : { kind: 'unavailable', reason: 'pane-unsearchable' }
}
