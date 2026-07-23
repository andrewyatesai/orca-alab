// Production wiring for the remote/SSH federated source (§2.4, Wave-5 5C): the
// `terminal.search` call over the runtime-environment route, plus remote-pane
// enumeration.
//
// The call side drives the LANDED 5B wire (runtime `terminal.search`, dispatched
// to the runtime route or the SSH channel multiplexer, schema-versioned with
// old-host degradation). Enumeration reads the remote federated-pane registry —
// which the remote-runtime PTY transport populates at construction (the "wired
// where the remote pty transport is owned" seam) — and joins it with store tab
// identity, exactly mirroring live-pane-discovery's store index. Each binding
// resolves its own environment/handle/session and its REPLAYED-anchor geometry
// (multiplexer anchor + replay origin/row-count/cols); a pane with no in-window
// anchor still enumerates and degrades to inline-only via the adapter.

import {
  isRemoteTerminalSearchResultShape,
  type RemoteTerminalSearchResult
} from '../../../../shared/terminal-remote-search-protocol'
import { useAppStore } from '@/store'
import { listRemoteFederatedPaneBindings } from './remote-federated-pane-registry'
import type { DiscoveredRemotePane, RemoteSearchCall } from './remote-pane-search-adapter'

type StoreTabIndexEntry = { worktreeId: string; title: string | null }

/** Store tab identity by tabId (worktree + title), mirroring live-pane-discovery
 *  so remote panes carry the same paneRef provenance local panes do. */
function indexTabs(): Map<string, StoreTabIndexEntry> {
  const state = useAppStore.getState()
  const byTabId = new Map<string, StoreTabIndexEntry>()
  for (const [worktreeId, tabs] of Object.entries(state.tabsByWorktree)) {
    for (const tab of tabs) {
      byTabId.set(tab.id, { worktreeId, title: tab.title ?? null })
    }
  }
  return byTabId
}

/** Enumerate live remote/SSH panes with resolved client replay geometry. Reads
 *  the transport-populated registry and joins store tab identity; a registered
 *  pane whose transport has not resolved its runtime/host yet (pre-connect), or
 *  whose tab is gone, is skipped for this generation's fan-out. */
export function discoverRemoteFederatedPanes(): DiscoveredRemotePane[] {
  const tabIndex = indexTabs()
  const panes: DiscoveredRemotePane[] = []
  for (const { paneKey, binding } of listRemoteFederatedPaneBindings()) {
    const environmentId = binding.environmentId()
    const hostTerminalId = binding.hostTerminalId()
    // Not yet connected to a host terminal → no searchable target this pass.
    if (!environmentId || !hostTerminalId) {
      continue
    }
    const tabMeta = tabIndex.get(binding.tabId)
    // Tab torn down but the transport has not disposed yet → skip, not a wrong
    // paneRef.
    if (!tabMeta) {
      continue
    }
    const geometry = binding.replayGeometry()
    panes.push({
      paneRef: {
        tabId: binding.tabId,
        leafId: binding.leafId,
        paneKey,
        worktreeId: tabMeta.worktreeId,
        title: tabMeta.title
      },
      sessionId: binding.sessionId(),
      replayedAnchor: geometry ? geometry.replayedAnchor : null,
      replayOriginRow: geometry?.replayOriginRow ?? 0,
      replayedRowCount: geometry?.replayedRowCount ?? 0,
      clientCols: geometry?.clientCols ?? null,
      // Recency ordering is the live adapter's remaining deferral too; 0 keeps
      // remote groups ordered by their match rows, never a fabricated time.
      lastOutputAt: 0,
      environmentId,
      hostTerminalId
    })
  }
  return panes
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
