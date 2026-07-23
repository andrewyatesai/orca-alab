// Client side of the v1021 daemon federated-search RPCs (fed design §2.2/§2.3,
// Wave-4 4E groundwork): typed wrappers over the control-socket request channel,
// version-skew degradation, and the cold-session needsContent handshake against
// the daemon's checkpoint-generation-keyed replay cache. The Wave-5 parked and
// daemon source adapters compose these; nothing here touches the renderer.

import { SESSION_SEARCH_PROTOCOL_VERSION } from './daemon-protocol-versions'
import type { TerminalCheckpointFile } from './daemon-checkpoint-file'
import type { TerminalHistoryLogContents } from './terminal-history-log'

export type DaemonSearchMatch = {
  /** 0-based from the oldest retained history row; visible rows follow history. */
  absRow: number
  /** Char offsets into `line` (daemon rows are text-only; flagged approximate). */
  col: number
  len: number
  line: string
}

export type DaemonSessionSearchHit = {
  sessionId: string
  matches: DaemonSearchMatch[]
  total: number
  incomplete: boolean
}

export type DaemonSearchQueryOptions = {
  query: string
  caseSensitive?: boolean
  regex?: boolean
}

/** The minimal control-socket seam (satisfied by DaemonClient / the pty
 *  adapter's request path); `protocolVersion` is the version the transport
 *  NEGOTIATED, so a preserved older daemon degrades instead of erroring. */
export type DaemonSearchTransport = {
  protocolVersion: number
  request<T = unknown>(type: string, payload: unknown): Promise<T>
}

export function daemonSearchSupported(protocolVersion: number): boolean {
  return protocolVersion >= SESSION_SEARCH_PROTOCOL_VERSION
}

// Why: a preserved daemon can be older than its negotiated version constants
// suggest mid-rollout; its "unsupported request type" reply must read as
// source-unavailable, never as a failed federated query.
function isUnsupportedRequestError(error: unknown): boolean {
  return error instanceof Error && error.message.includes('unsupported request type')
}

export type DaemonSessionsSearchResult = {
  /** False when this daemon cannot search (old protocol) — the federated
   *  controller shows the source as unavailable rather than empty. */
  available: boolean
  sessions: DaemonSessionSearchHit[]
}

/** `searchSessions` over WARM daemon sessions. `sessionIds` is the controller's
 *  dedup allowlist; `cutoffRows[sid]` is the depth-extension cutoff (only rows
 *  strictly older than the live pane's window report). Summaries only. */
export async function searchDaemonSessions(
  transport: DaemonSearchTransport,
  opts: DaemonSearchQueryOptions & {
    sessionIds?: string[]
    cutoffRows?: Record<string, number>
    maxPerSession?: number
    gen?: number
  }
): Promise<DaemonSessionsSearchResult> {
  if (!daemonSearchSupported(transport.protocolVersion)) {
    return { available: false, sessions: [] }
  }
  try {
    const payload = await transport.request<{ sessions?: DaemonSessionSearchHit[] }>(
      'searchSessions',
      {
        query: opts.query,
        caseSensitive: opts.caseSensitive ?? false,
        regex: opts.regex ?? false,
        ...(opts.sessionIds ? { sessionIds: opts.sessionIds } : {}),
        ...(opts.cutoffRows ? { cutoffRows: opts.cutoffRows } : {}),
        ...(opts.maxPerSession !== undefined ? { maxPerSession: opts.maxPerSession } : {}),
        ...(opts.gen !== undefined ? { gen: opts.gen } : {})
      }
    )
    return { available: true, sessions: payload?.sessions ?? [] }
  } catch (error) {
    if (isUnsupportedRequestError(error)) {
      return { available: false, sessions: [] }
    }
    throw error
  }
}

export type DaemonSearchContextWindow = {
  lines: string[]
  firstAbsRow: number
}

/** `searchContext` — the ±N-line inline expansion window for a WARM session.
 *  Null when the session is gone (expected staleness, not an error). */
export async function fetchDaemonSearchContext(
  transport: DaemonSearchTransport,
  opts: { sessionId: string; absRow: number; before?: number; after?: number }
): Promise<DaemonSearchContextWindow | null> {
  if (!daemonSearchSupported(transport.protocolVersion)) {
    return null
  }
  try {
    return await transport.request<DaemonSearchContextWindow>('searchContext', opts)
  } catch (error) {
    if (isUnsupportedRequestError(error) || (error instanceof Error && error.message.includes('unknown session'))) {
      return null
    }
    throw error
  }
}

/** Replay content for a DEAD (persisted-only) session or a parked snapshot:
 *  ANSI chunks the daemon feeds through a transient headless parse (the
 *  policy-mandated Rust strip — never a TS regex strip). */
export type DeadSessionReplayContent = {
  rows: number
  cols: number
  scrollbackRows?: number
  chunks: string[]
}

// Why 50k: the daemon clamps scrollbackRows to the app's policy ceiling; ask
// for the maximum so a 5MB log replays with the deepest searchable history.
const REPLAY_SCROLLBACK_ROWS = 50_000

/** Assemble replay chunks from persisted checkpoint + incremental log,
 *  mirroring history-reader's cold-restore ordering (alt-screen scrollback
 *  rule, generation pairing). `resize` records are skipped — search rows are
 *  wrap-width-approximate by contract; `clear` maps to ED3, which the engine
 *  applies as a scrollback clear. */
export function buildReplaySearchContent(
  checkpoint: TerminalCheckpointFile | null,
  log: TerminalHistoryLogContents | null,
  fallbackSize: { cols: number; rows: number }
): DeadSessionReplayContent {
  const chunks: string[] = []
  if (checkpoint) {
    // Why alt-only: on a normal screen snapshotAnsi already prepends the
    // history — adding scrollbackAnsi would double every line (history-reader).
    if (checkpoint.modes?.alternateScreen && checkpoint.scrollbackAnsi) {
      chunks.push(checkpoint.scrollbackAnsi)
    }
    if (checkpoint.rehydrateSequences) {
      chunks.push(checkpoint.rehydrateSequences)
    }
    if (checkpoint.snapshotAnsi) {
      chunks.push(checkpoint.snapshotAnsi)
    }
  }
  // Why the pairing check: a log whose header generation doesn't match the
  // checkpoint belongs to a superseded epoch; replaying it would duplicate or
  // interleave stale bytes (same rule as restoreFromIncrementalLog).
  const logMatchesCheckpoint = checkpoint
    ? log?.generation === (checkpoint.generation ?? 0)
    : log?.generation === 0
  if (log && logMatchesCheckpoint) {
    for (const batch of log.batches) {
      for (const record of batch.records) {
        if (record.kind === 'output') {
          chunks.push(record.data)
        } else if (record.kind === 'clear') {
          chunks.push('\x1b[3J')
        }
      }
    }
  }
  return {
    rows: checkpoint?.rows ?? fallbackSize.rows,
    cols: checkpoint?.cols ?? fallbackSize.cols,
    scrollbackRows: REPLAY_SCROLLBACK_ROWS,
    chunks
  }
}

type ReplaySearchResult = {
  matches: DaemonSearchMatch[]
  total: number
  incomplete: boolean
}

/** Drive one replay-backed RPC through the needsContent handshake: try the
 *  generation-keyed cache first (no bytes on the wire), ship the stored ANSI
 *  once on a miss, and never loop — a second needsContent is a daemon bug
 *  surfaced as null rather than an infinite resend. */
async function requestWithReplayHandshake<T extends { needsContent?: boolean }>(
  transport: DaemonSearchTransport,
  type: string,
  payload: Record<string, unknown>,
  loadContent: () => Promise<DeadSessionReplayContent | null>
): Promise<T | null> {
  if (!daemonSearchSupported(transport.protocolVersion)) {
    return null
  }
  try {
    const first = await transport.request<T>(type, payload)
    if (!first?.needsContent) {
      return first
    }
    const content = await loadContent()
    if (!content) {
      return null
    }
    const second = await transport.request<T>(type, { ...payload, content })
    return second?.needsContent ? null : second
  } catch (error) {
    if (isUnsupportedRequestError(error)) {
      return null
    }
    throw error
  }
}

/** `searchReplay` — search a dead session's persisted history (or a parked
 *  snapshot) daemon-side. `generation` is the checkpoint generation; content
 *  loads lazily and crosses the socket at most once per generation. */
export async function searchDeadSessionHistory(
  transport: DaemonSearchTransport,
  opts: DaemonSearchQueryOptions & {
    sessionId: string
    generation: number
    maxMatches?: number
    cutoffRow?: number
    loadContent: () => Promise<DeadSessionReplayContent | null>
  }
): Promise<ReplaySearchResult | null> {
  return requestWithReplayHandshake<ReplaySearchResult & { needsContent?: boolean }>(
    transport,
    'searchReplay',
    {
      sessionId: opts.sessionId,
      generation: opts.generation,
      query: opts.query,
      caseSensitive: opts.caseSensitive ?? false,
      regex: opts.regex ?? false,
      ...(opts.maxMatches !== undefined ? { maxMatches: opts.maxMatches } : {}),
      ...(opts.cutoffRow !== undefined ? { cutoffRow: opts.cutoffRow } : {})
    },
    opts.loadContent
  )
}

/** `searchReplayContext` — the dead-session inline context expansion (no pane
 *  exists; identity is the sessionId). Same handshake and cache. */
export async function fetchDeadSessionSearchContext(
  transport: DaemonSearchTransport,
  opts: {
    sessionId: string
    generation: number
    absRow: number
    before?: number
    after?: number
    loadContent: () => Promise<DeadSessionReplayContent | null>
  }
): Promise<DaemonSearchContextWindow | null> {
  return requestWithReplayHandshake<DaemonSearchContextWindow & { needsContent?: boolean }>(
    transport,
    'searchReplayContext',
    {
      sessionId: opts.sessionId,
      generation: opts.generation,
      absRow: opts.absRow,
      ...(opts.before !== undefined ? { before: opts.before } : {}),
      ...(opts.after !== undefined ? { after: opts.after } : {})
    },
    opts.loadContent
  )
}
