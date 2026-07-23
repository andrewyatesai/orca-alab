// The client-side registry of live remote/SSH panes eligible for federated
// search (FEDERATED-SEARCH-DESIGN §2.4). This is the "wired where the remote
// pty transport is owned" seam the remote-pane-discovery header defers to: each
// remote-runtime PTY transport self-registers a binding here on construction and
// removes it on destroy, so enumeration reads ACTUAL connected panes rather than
// walking the pane-manager (whose panes hold no transport reference).
//
// Bindings expose LIVE getters (never frozen snapshots) so a reconnect, a handle
// re-derivation, or a fresh replayed anchor is reflected without re-registration.

import type { ReplayedSearchGeometry } from '../../components/terminal-pane/pty-transport-types'

/** One live remote pane's federated-search inputs, read on demand from the
 *  owning transport (env/handle/session) and its multiplexer (replay geometry). */
export type RemoteFederatedPaneBinding = {
  tabId: string
  leafId: string
  /** Runtime-environment selector routing `terminal.search` to this pane's host
   *  (null before the transport resolves its runtime). */
  environmentId: () => string | null
  /** Raw host terminal handle the `terminal.search` request names (null before
   *  the transport has attached/subscribed to a host terminal). */
  hostTerminalId: () => string | null
  /** Env-qualified remote PTY id — a stable cross-view dedup key so two client
   *  panes viewing the SAME host terminal merge into one federated group. Null
   *  before connect. */
  sessionId: () => string | null
  /** Frozen client replay geometry cross-checked against the live multiplexer
   *  anchor, or null when no anchored snapshot is currently replayed (skew). */
  replayGeometry: () => ReplayedSearchGeometry | null
}

const bindings = new Map<string, RemoteFederatedPaneBinding>()

/** Register a remote pane's binding under its stable paneKey. Returns an
 *  idempotent unregister the transport calls on destroy; a re-register under the
 *  same key (transport rebuild) replaces the prior binding. */
export function registerRemoteFederatedPane(
  paneKey: string,
  binding: RemoteFederatedPaneBinding
): () => void {
  bindings.set(paneKey, binding)
  return () => {
    // Only remove OUR binding — a newer transport may already own this paneKey.
    if (bindings.get(paneKey) === binding) {
      bindings.delete(paneKey)
    }
  }
}

/** Snapshot the currently registered remote panes for one enumeration pass. */
export function listRemoteFederatedPaneBindings(): {
  paneKey: string
  binding: RemoteFederatedPaneBinding
}[] {
  return [...bindings.entries()].map(([paneKey, binding]) => ({ paneKey, binding }))
}

/** Test seam: drop all registrations between cases. */
export function clearRemoteFederatedPaneBindings(): void {
  bindings.clear()
}
