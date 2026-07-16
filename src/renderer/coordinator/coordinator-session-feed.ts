// The coordinator's one data source: a DaemonProtocolClient over the preload
// byte tunnel. Control socket polls listSessions/getForegroundProcess and
// issues read-only `subscribe` attaches (protocol 1019 — never createOrAttach,
// which would steal ownership from Orca panes); the stream socket's data/exit
// events keep per-session tails and activity times fresh. React consumes it
// via useSyncExternalStore.
import { useSyncExternalStore } from 'react'
import { DaemonProtocolClient, type DaemonStreamEvent } from '../../shared/daemon-protocol-client'
import { openCoordinatorDaemonTransport } from './daemon-tunnel-transport'
import { appendBoundedTail } from './terminal-text-preview'
import {
  createSessionByteTaps,
  seedForEngineReplay,
  type RawByteSink
} from './coordinator-raw-byte-tap'

export type CoordinatorSessionView = {
  sessionId: string
  title: string
  isAlive: boolean
  exitCode: number | null
  foregroundProcess: string | null
  createdAt: number
  lastActivityAt: number
  /** Raw ANSI tail (hydration snapshot + live data), bounded; callers strip at
   *  render time (terminal-text-preview.ts). */
  ansiTail: string
}

export type CoordinatorConnection =
  | { state: 'connecting' }
  | { state: 'connected' }
  | { state: 'error'; message: string }

export type CoordinatorFeedSnapshot = {
  connection: CoordinatorConnection
  sessions: CoordinatorSessionView[]
}

type SessionInfoLine = {
  sessionId: string
  isAlive: boolean
  createdAt: number
}

type SubscribePayload = {
  snapshot: {
    snapshotAnsi: string
    scrollbackAnsi: string
    /** Mode re-seed sequences (DECSET etc.) the engine replay needs before the
     *  live frame; the text preview strips them, so both consumers share one tail. */
    rehydrateSequences?: string
    /** Dangling mid-escape tail; replayed LAST so the next live chunk completes
     *  it instead of it printing literally (Bug E / #7329 ordering). */
    pendingEscapeTailAnsi?: string
    lastTitle?: string
  } | null
}

const LIST_POLL_MS = 2000
const FOREGROUND_POLL_MS = 3000
const RECONNECT_DELAY_MS = 3000
// ~200 rows of dense output; enough for the focused view's text tail.
const ANSI_TAIL_MAX_CHARS = 64_000

class CoordinatorSessionFeed {
  #sessions = new Map<string, CoordinatorSessionView>()
  #subscribed = new Set<string>()
  #connection: CoordinatorConnection = { state: 'connecting' }
  #listeners = new Set<() => void>()
  #client: DaemonProtocolClient | null = null
  #snapshot: CoordinatorFeedSnapshot = {
    connection: this.#connection,
    sessions: []
  }
  #started = false
  #timers: ReturnType<typeof setTimeout>[] = []
  // Live raw-byte fan-out for focused aterm tiles; retention stays the ONE
  // bounded ansiTail — a tap only seeds from it and then streams pass-through.
  #byteTaps = createSessionByteTaps()

  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener)
    this.start()
    return () => this.#listeners.delete(listener)
  }

  getSnapshot = (): CoordinatorFeedSnapshot => this.#snapshot

  start(): void {
    if (this.#started) {
      return
    }
    this.#started = true
    void this.#connectLoop()
  }

  async #connectLoop(): Promise<void> {
    this.#setConnection({ state: 'connecting' })
    const client = new DaemonProtocolClient({
      // Unique per window lifetime so a reload never collides with the dead
      // client's daemon-side entry.
      clientId: `coordinator-${Date.now().toString(36)}`,
      openTransport: openCoordinatorDaemonTransport
    })
    try {
      await client.connect()
    } catch (error) {
      client.close()
      this.#setConnection({
        state: 'error',
        message: error instanceof Error ? error.message : String(error)
      })
      this.#timers.push(setTimeout(() => void this.#connectLoop(), RECONNECT_DELAY_MS))
      return
    }
    this.#client = client
    this.#subscribed.clear()
    client.onEvent((event) => this.#onStreamEvent(event))
    client.onClose(() => {
      // The daemon (or tunnel) went away: drop and re-enter the connect loop —
      // the daemon owns the sessions, so reconnect IS the recovery flow.
      this.#client = null
      this.#setConnection({
        state: 'error',
        message: 'daemon connection lost — reconnecting'
      })
      this.#timers.push(setTimeout(() => void this.#connectLoop(), RECONNECT_DELAY_MS))
    })
    this.#setConnection({ state: 'connected' })
    void this.#pollSessions()
    void this.#pollForegroundProcesses()
  }

  async #pollSessions(): Promise<void> {
    const client = this.#client
    if (!client) {
      return
    }
    try {
      const response = await client.rpc<{ sessions: SessionInfoLine[] }>('listSessions')
      if (response.ok) {
        await this.#reconcileSessionList(client, response.payload.sessions)
      }
    } catch {
      // Connection loss is handled by onClose; skip this tick.
    }
    if (this.#client === client) {
      this.#timers.push(setTimeout(() => void this.#pollSessions(), LIST_POLL_MS))
    }
  }

  async #reconcileSessionList(
    client: DaemonProtocolClient,
    listed: SessionInfoLine[]
  ): Promise<void> {
    const listedIds = new Set(listed.map((info) => info.sessionId))
    for (const info of listed) {
      const existing = this.#sessions.get(info.sessionId)
      if (!existing) {
        this.#sessions.set(info.sessionId, {
          sessionId: info.sessionId,
          title: info.sessionId,
          isAlive: info.isAlive,
          exitCode: null,
          foregroundProcess: null,
          createdAt: info.createdAt,
          lastActivityAt: info.createdAt || Date.now(),
          ansiTail: ''
        })
      } else if (!existing.isAlive && info.isAlive) {
        this.#patch(info.sessionId, { isAlive: true, exitCode: null })
      }
      if (info.isAlive && !this.#subscribed.has(info.sessionId)) {
        this.#subscribed.add(info.sessionId)
        await this.#hydrateSubscription(client, info.sessionId)
      }
    }
    // Gone from the daemon without an exit event (reaped earlier): mark ended.
    for (const [sessionId, view] of this.#sessions) {
      if (view.isAlive && !listedIds.has(sessionId)) {
        this.#patch(sessionId, { isAlive: false })
      }
    }
    this.#emit()
  }

  async #hydrateSubscription(client: DaemonProtocolClient, sessionId: string): Promise<void> {
    try {
      const response = await client.rpc<SubscribePayload>('subscribe', {
        sessionId
      })
      if (!response.ok) {
        this.#subscribed.delete(sessionId)
        return
      }
      const snapshot = response.payload.snapshot
      if (snapshot) {
        // Engine replay order (mirrors the reattach path): history, mode
        // re-seed, live frame, dangling escape tail LAST — the next live data
        // chunk carries its continuation bytes.
        const hydrated = `${snapshot.scrollbackAnsi}\n${snapshot.rehydrateSequences ?? ''}${snapshot.snapshotAnsi}${snapshot.pendingEscapeTailAnsi ?? ''}`
        this.#patch(sessionId, {
          ansiTail: appendBoundedTail('', hydrated, ANSI_TAIL_MAX_CHARS),
          ...(snapshot.lastTitle ? { title: snapshot.lastTitle } : {})
        })
      }
    } catch {
      this.#subscribed.delete(sessionId)
    }
  }

  async #pollForegroundProcesses(): Promise<void> {
    const client = this.#client
    if (!client) {
      return
    }
    for (const view of this.#sessions.values()) {
      if (!view.isAlive) {
        continue
      }
      try {
        const response = await client.rpc<{ foregroundProcess: string | null }>(
          'getForegroundProcess',
          { sessionId: view.sessionId }
        )
        if (response.ok) {
          this.#patch(view.sessionId, {
            foregroundProcess: response.payload.foregroundProcess
          })
        }
      } catch {
        break
      }
    }
    this.#emit()
    if (this.#client === client) {
      this.#timers.push(setTimeout(() => void this.#pollForegroundProcesses(), FOREGROUND_POLL_MS))
    }
  }

  #onStreamEvent(event: DaemonStreamEvent): void {
    const view = this.#sessions.get(event.sessionId)
    if (!view) {
      return
    }
    if (event.event === 'data') {
      const data = typeof event.payload?.data === 'string' ? event.payload.data : ''
      this.#patch(event.sessionId, {
        ansiTail: appendBoundedTail(view.ansiTail, data, ANSI_TAIL_MAX_CHARS),
        lastActivityAt: Date.now()
      })
      // Retention first, then fan-out: a tap's seed (the tail) + its live
      // stream then always compose the same byte order the tail retained.
      this.#byteTaps.deliver(event.sessionId, data)
      this.#emit()
    } else if (event.event === 'exit') {
      const code = typeof event.payload?.code === 'number' ? event.payload.code : null
      this.#subscribed.delete(event.sessionId)
      this.#patch(event.sessionId, {
        isAlive: false,
        exitCode: code,
        lastActivityAt: Date.now()
      })
      this.#emit()
    }
  }

  tapSessionBytes(sessionId: string, sink: RawByteSink): () => void {
    // Seed with the bounded retained tail (resynced if it was ever sliced) so
    // the engine paints history immediately; live chunks continue the stream.
    const view = this.#sessions.get(sessionId)
    const seed = view ? seedForEngineReplay(view.ansiTail, ANSI_TAIL_MAX_CHARS) : ''
    if (seed) {
      sink(seed)
    }
    return this.#byteTaps.add(sessionId, sink)
  }

  #patch(sessionId: string, patch: Partial<CoordinatorSessionView>): void {
    const view = this.#sessions.get(sessionId)
    if (view) {
      this.#sessions.set(sessionId, { ...view, ...patch })
    }
  }

  #setConnection(connection: CoordinatorConnection): void {
    this.#connection = connection
    this.#emit()
  }

  #emit(): void {
    this.#snapshot = {
      connection: this.#connection,
      sessions: [...this.#sessions.values()].sort((a, b) => b.createdAt - a.createdAt)
    }
    for (const listener of this.#listeners) {
      listener()
    }
  }
}

// Module-level singleton: StrictMode double-mounts must share one daemon
// client, and the feed survives component remounts (the daemon owns state).
const feed = new CoordinatorSessionFeed()

export function useCoordinatorSessionFeed(): CoordinatorFeedSnapshot {
  return useSyncExternalStore(feed.subscribe, feed.getSnapshot)
}

/** Tap a session's raw PTY byte stream for a focused aterm tile: seeds with the
 *  bounded retained tail, then forwards live chunks. Returns the untap disposer. */
export function tapCoordinatorSessionBytes(sessionId: string, sink: RawByteSink): () => void {
  return feed.tapSessionBytes(sessionId, sink)
}
