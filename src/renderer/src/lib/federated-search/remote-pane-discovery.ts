// Production wiring for the remote/SSH federated source (§2.4, Wave-5 5C): the
// `terminal.search` call over the runtime-environment route, plus remote-pane
// enumeration.
//
// The call side is complete — it drives the LANDED 5B wire (runtime `terminal.search`,
// which the main process dispatches to the runtime route or the SSH channel
// multiplexer, schema-versioned with old-host degradation). Enumeration is the
// deferred integration step (same deferral style as live-pane-discovery's
// `sessionId: null` and productionJumpDeps' `expandDaemonInline: () => false`):
// resolving each remote pane's REPLAYED-anchor geometry
// (`multiplexer.getReplayedHostAnchor()` + replay origin/row-count/cols) is wired
// where the remote pty transport is owned, at pin time. Returning [] keeps the
// remote adapter registered and inert until then — never a wrong result.

import {
  isRemoteTerminalSearchResultShape,
  type RemoteTerminalSearchResult
} from '../../../../shared/terminal-remote-search-protocol'
import type { DiscoveredRemotePane, RemoteSearchCall } from './remote-pane-search-adapter'

/** Enumerate remote/SSH panes with resolved client replay geometry. Deferred:
 *  see the module header — returns [] until the remote-transport owner supplies
 *  each pane's multiplexer anchor + replay geometry at pin time. */
export function discoverRemoteFederatedPanes(): DiscoveredRemotePane[] {
  return []
}

type RuntimeCall = (args: {
  selector?: string
  method: string
  params?: unknown
  timeoutMs?: number
}) => Promise<unknown>

/** The runtime-environment RPC entry, or null in environments without it. */
function runtimeCall(): RuntimeCall | null {
  const api = (globalThis as { api?: { runtimeEnvironments?: { call?: unknown } } }).api
  const call = api?.runtimeEnvironments?.call
  return typeof call === 'function' ? (call as RuntimeCall) : null
}

/** Unwrap the runtime RPC envelope defensively — any non-ok / unexpected shape
 *  becomes null so the adapter reports the source absent, never an error. */
function unwrapSearchResult(response: unknown): RemoteTerminalSearchResult | null {
  if (!response || typeof response !== 'object') {
    return null
  }
  const envelope = response as { ok?: unknown; result?: unknown }
  const candidate = envelope.ok === true && 'result' in envelope ? envelope.result : response
  return isRemoteTerminalSearchResultShape(candidate) ? candidate : null
}

/** Production `terminal.search` call over the 5B wire (runtime route + SSH mux,
 *  both reached through the runtime-environment selector). Cancellation-aware:
 *  a raced abort drops the result rather than surfacing a stale batch. */
export const productionRemoteSearchCall: RemoteSearchCall = async (pane, request, signal) => {
  const call = runtimeCall()
  if (!call) {
    return null
  }
  let response: unknown
  try {
    response = await call({
      selector: pane.environmentId,
      method: 'terminal.search',
      params: {
        terminal: pane.hostTerminalId,
        query: request.query,
        caseSensitive: request.caseSensitive,
        regex: request.isRegex,
        maxMatches: request.maxMatches,
        gen: request.gen,
        ...(request.clientAnchorGen !== undefined
          ? { clientAnchorGen: request.clientAnchorGen }
          : {})
      },
      timeoutMs: 8_000
    })
  } catch {
    return null // method-not-found / unreachable → source absent
  }
  if (signal.aborted) {
    return null
  }
  return unwrapSearchResult(response)
}
