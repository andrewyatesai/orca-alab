import type { ParsedAgentStatusPayload } from '../../../../shared/agent-status-types'
import type { SleepingAgentLaunchConfig } from '../../../../shared/agent-session-resume'
import type { StartupCommandDelivery } from '../../../../shared/codex-startup-delivery'
import type { ProjectExecutionRuntimeResolution } from '../../../../shared/project-execution-runtime'
import type { EventProps } from '../../../../shared/telemetry-events'
import type { TerminalOscColorQueryReplyColors } from '../../../../shared/terminal-osc-color-reply'
import type { TuiAgent } from '../../../../shared/types'
import type { PtyDataMeta } from './pty-dispatcher'

export type PtyBufferSnapshot = {
  data: string
  cols: number
  rows: number
  seq?: number
  /** Lowest seq main could still deliver when the snapshot was taken (start
   *  of its pending renderer-delivery queue; equals `seq` when empty). Bytes
   *  are delivered once and in order, so a post-restore chunk at or below
   *  this seq can never be a duplicate the snapshot already covers. */
  pendingDeliveryStartSeq?: number
  source?: 'headless' | 'renderer'
  /** True when the snapshot captures an alternate-screen TUI (Claude Code,
   *  vim). Restore must NOT clear xterm's buffer in that case — the TUI's
   *  scrollback lives in xterm and a clear destroys scroll-up after a tab
   *  return. Mirrors the attach-time guard in pty-transport.ts. */
  alternateScreen?: boolean
  /** Authoritative normal buffer paired with an alternate-screen frame. */
  scrollbackAnsi?: string
  /** Trailing incomplete escape sequence main's emulator ingested (a PTY read
   *  ended mid-escape). Must be written LAST — after post-replay resets, right
   *  before post-snapshot live chunks — so the continuation completes it
   *  exactly as live instead of rendering literal (Bug E / #7329). */
  pendingEscapeTailAnsi?: string
}

export type LocalPtySessionMetadata = {
  cwd?: string
  shellOverride?: string
}

/** Client-side replay geometry a remote transport froze at the last ENGINE-
 *  replayed snapshot, paired with the multiplexer anchor still in force (fed
 *  §2.4). Feeds the remote federated-search row remap. Null when no anchored
 *  snapshot is currently replayed (skew/reset) — the search degrades to
 *  inline-only rather than remapping against a window the engine no longer holds. */
export type ReplayedSearchGeometry = {
  /** Anchor of the snapshot the client actually replayed (matches the live
   *  multiplexer anchor; a mismatch collapses this whole value to null). */
  replayedAnchor: { hostRowAnchor: number; anchorGen: number }
  /** Client engine row where the replayed snapshot's first row landed. */
  replayOriginRow: number
  /** Rows the client replayed (history + viewport) from that snapshot. */
  replayedRowCount: number
  /** Client engine grid width — differing widths flag the jump approximate. */
  clientCols: number
}

export type PtyConnectResult = {
  id: string
  /** The requested session exited while it had no primary pane handler. Its
   *  buffered final data/exit were delivered, so callers must not fresh-spawn. */
  exitedBeforeAttach?: boolean
  launchAgent?: TuiAgent
  launchConfig?: SleepingAgentLaunchConfig
  snapshot?: string
  snapshotCols?: number
  snapshotRows?: number
  isAlternateScreen?: boolean
  sessionExpired?: boolean
  coldRestore?: {
    scrollback: string
    cwd: string
    cols?: number
    rows?: number
    /** Last command recovered from the crashed session's log (#7596). */
    lastCommand?: string
  }
  replay?: string
  startupCwdFallback?: { kind: 'worktree'; cwd: string }
  /** Trailing partial escape the daemon emulator held mid-parse; the reattach
   *  replay writes it LAST (after the reset) so a racing live continuation
   *  completes it instead of rendering literally (#7329). */
  pendingEscapeTailAnsi?: string
  // Why: the renderer asked to reattach but the daemon found the session gone
  // and spawned a fresh one (no snapshot/replay/coldRestore). Signals
  // handleReattachResult to clear the stale frame restoreScrollbackBuffers
  // painted at mount, so the fresh shell does not start under dead output.
  respawnedFresh?: boolean
}

type PtyCallbacks = {
  onConnect?: () => void
  onDisconnect?: () => void
  onData?: (data: string, meta?: PtyDataMeta) => void
  onReplayData?: (
    data: string,
    meta?: { clearBeforeReplay?: boolean; pendingEscapeTailAnsi?: string }
  ) => void
  onStatus?: (shell: string) => void
  onError?: (message: string, errors?: string[]) => void
  onExit?: (code: number) => void
  /** Remote-runtime only: the initial subscribe snapshot exceeded the client
   *  replay limit and was dropped (old host without the subscribe budget); the
   *  pane should restore via the server-bounded requested-snapshot path. */
  onSnapshotOverflow?: () => void
}

export type PtyTransport = {
  connect: (options: {
    url: string
    cols?: number
    rows?: number
    sessionId?: string
    /** Hidden-at-spawn declaration (terminal-query-authority.md): no visible
     *  view will consume this PTY's bytes, so main marks it hidden BEFORE the
     *  first byte and the gate + model responder own spawn-time queries.
     *  Ignored by remote-runtime transports (not gate-markable). */
    initiallyHidden?: boolean
    command?: string
    env?: Record<string, string>
    launchConfig?: SleepingAgentLaunchConfig
    launchToken?: string
    launchAgent?: TuiAgent
    startupCommandDelivery?: StartupCommandDelivery
    callbacks: PtyCallbacks
  }) => void | Promise<void | string | PtyConnectResult>
  attach: (options: {
    existingPtyId: string
    cols?: number
    rows?: number
    isAlternateScreen?: boolean
    callbacks: PtyCallbacks
  }) => void
  disconnect: () => void
  sendInput: (data: string) => boolean
  // Why: latency-critical terminal query replies (CPR/DSR/DA/OSC color/pixel
  // size) must skip input coalescing — a querying program reads them in raw
  // mode with a short timeout, so a debounced reply lands on the shell prompt
  // and corrupts input (#7329). Local transports already write promptly, so
  // this is `sendInput` for them; the remote transport flushes pending input
  // (preserving order) and sends the reply immediately.
  sendInputImmediate: (data: string) => boolean
  sendInputAccepted?: (data: string) => Promise<boolean>
  claimViewport?: (cols: number, rows: number) => boolean
  resize: (
    cols: number,
    rows: number,
    meta?: {
      widthPx?: number
      heightPx?: number
      cellW?: number
      cellH?: number
      claim?: boolean
    }
  ) => boolean
  isConnected: () => boolean
  getPtyId: () => string | null
  getConnectionId?: () => string | null | undefined
  /** The runtime captured by this transport; legacy remote PTY ids do not
   * encode their owner, and current worktree settings may have changed. */
  getRuntimeEnvironmentId?: () => string | null
  /** Remote-runtime only: the client replay geometry for the federated remote
   *  search row remap (fed §2.4), or null when no anchored snapshot is currently
   *  replayed (skew/reset — the remap degrades to inline-only). */
  getReplayedSearchGeometry?: () => ReplayedSearchGeometry | null
  /** This view's identity in the #9156 query-reply authority election: the
   *  remote subscribe clientId for remote-viewer transports, absent/null for
   *  host (IPC) transports. */
  getQueryReplyViewerClientId?: () => string | null
  getLocalSessionMetadata?: () => LocalPtySessionMetadata | null
  /** Drop cross-chunk parser carries (partial OSC-9999 prefix). Called when a
   *  model-restore marker reports dropped bytes — a carry spanning the gap
   *  would corrupt the next live chunk. IPC transports only. */
  resetCrossChunkParserState?: () => void
  serializeBuffer?: (opts?: { scrollbackRows?: number }) => Promise<PtyBufferSnapshot | null>
  preserve?: () => void
  detach?: () => void
  destroy?: () => void | Promise<void>
}

export type IpcPtyTransportOptions = {
  cwd?: string
  cwdFallback?: 'worktree'
  env?: Record<string, string>
  command?: string
  launchConfig?: SleepingAgentLaunchConfig
  launchToken?: string
  launchAgent?: TuiAgent
  startupCommandDelivery?: StartupCommandDelivery
  connectionId?: string | null
  worktreeId?: string
  tabId?: string
  leafId?: string
  activate?: boolean
  shellOverride?: string
  projectRuntime?: ProjectExecutionRuntimeResolution
  terminalColorQueryReplies?: TerminalOscColorQueryReplyColors
  telemetry?: EventProps<'agent_started'>
  onPtyExit?: (ptyId: string, exitCode: number) => void
  onTitleChange?: (title: string, rawTitle: string) => void
  onPtySpawn?: (ptyId: string) => void
  /** Rebind an existing pane after its provider replaces the PTY identity. */
  onPtyRebind?: (ptyId: string, replacedPtyId: string) => void
  onBell?: () => void
  onAgentBecameIdle?: (title: string) => void
  onAgentBecameWorking?: () => void
  onAgentExited?: () => void
  onAgentStatus?: (payload: ParsedAgentStatusPayload) => void
  /** Remote-runtime only: reads the pane engine's live replay geometry (base_y,
   *  grid rows/cols) so the transport can freeze it against each engine-replayed
   *  snapshot anchor (fed §2.4 client side). Absent → remote federated search
   *  for this pane has no in-window remap and stays inline-only. */
  readClientReplayGeometry?: () => { baseY: number; rows: number; cols: number } | null
}
