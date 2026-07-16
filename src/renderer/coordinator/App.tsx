// Coordinator v0, milestone A: the session grid + attention queue + read-only
// focused view (docs/rust-migration/coordinator-v0-design.md). A calm surface:
// every card answers "what is this agent doing, and does it need me?".
import { useEffect, useState } from 'react'
import { Badge } from '@/components/ui/badge'
import { useCoordinatorSessionFeed, type CoordinatorConnection } from './coordinator-session-feed'
import { FocusedSessionView } from './focused-session-view'
import { deriveSessionStatus, orderAttentionQueue } from './session-status'
import { AttentionQueue, SessionTile, type SessionWithStatus } from './session-tiles'

// Re-render cadence for the "time since activity" labels only.
const CLOCK_TICK_MS = 5000

function useNowTick(): number {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), CLOCK_TICK_MS)
    return () => clearInterval(timer)
  }, [])
  return now
}

function ConnectionBadge({ connection }: { connection: CoordinatorConnection }): React.JSX.Element {
  if (connection.state === 'connected') {
    return (
      <Badge
        variant="outline"
        className="border-status-success-border bg-status-success-background text-status-success"
      >
        Connected
      </Badge>
    )
  }
  if (connection.state === 'connecting') {
    return <Badge variant="secondary">Connecting…</Badge>
  }
  return (
    <Badge variant="destructive" title={connection.message}>
      Daemon unavailable
    </Badge>
  )
}

export function App(): React.JSX.Element {
  const { connection, sessions } = useCoordinatorSessionFeed()
  const [focusedId, setFocusedId] = useState<string | null>(null)
  const nowMs = useNowTick()

  const views: SessionWithStatus[] = sessions.map((session) => ({
    ...session,
    status: deriveSessionStatus(session)
  }))
  const focused = views.find((view) => view.sessionId === focusedId) ?? null
  const queue = orderAttentionQueue(views)

  return (
    <div className="flex h-screen flex-col bg-background text-foreground">
      <header className="flex items-center gap-3 border-b border-border px-4 py-3">
        <h1 className="text-sm font-semibold">Coordinator</h1>
        <ConnectionBadge connection={connection} />
        <span className="ml-auto text-xs text-muted-foreground">
          {views.length === 1 ? '1 session' : `${views.length} sessions`}
        </span>
      </header>
      {focused ? (
        <FocusedSessionView session={focused} nowMs={nowMs} onBack={() => setFocusedId(null)} />
      ) : (
        <>
          <AttentionQueue sessions={queue} onFocus={setFocusedId} />
          <main className="scrollbar-sleek min-h-0 flex-1 overflow-y-auto">
            {views.length === 0 ? (
              <div className="flex h-full items-center justify-center">
                <p className="text-sm text-muted-foreground">
                  No sessions yet — terminals started in Orca appear here.
                </p>
              </div>
            ) : (
              <div className="grid gap-3 p-4 [grid-template-columns:repeat(auto-fill,minmax(320px,1fr))]">
                {views.map((view) => (
                  <SessionTile
                    key={view.sessionId}
                    session={view}
                    nowMs={nowMs}
                    onFocus={setFocusedId}
                  />
                ))}
              </div>
            )}
          </main>
        </>
      )}
    </div>
  )
}
