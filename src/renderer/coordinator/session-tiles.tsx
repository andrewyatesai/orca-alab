// The grid card + status chip + attention strip for coordinator v0
// (coordinator-v0-design.md §UI). Imports only ui primitives + main.css tokens.
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { cn } from '@/lib/utils'
import type { CoordinatorSessionView } from './coordinator-session-feed'
import { formatTimeSinceActivity, type CoordinatorSessionStatus } from './session-status'
import { terminalPlainTextTail } from './terminal-text-preview'

export type SessionWithStatus = CoordinatorSessionView & { status: CoordinatorSessionStatus }

const STATUS_LABELS: Record<CoordinatorSessionStatus, string> = {
  working: 'Working',
  'needs-you': 'Needs you',
  done: 'Done',
  failed: 'Failed',
  ended: 'Ended'
}

// Why: exhaustive switch with no default — a new status must pick its own
// presentation instead of silently inheriting one (never-silently-green).
export function StatusChip({ status }: { status: CoordinatorSessionStatus }): React.JSX.Element {
  switch (status) {
    case 'failed':
      return <Badge variant="destructive">{STATUS_LABELS.failed}</Badge>
    case 'done':
      return (
        <Badge
          variant="outline"
          className="border-status-success-border bg-status-success-background text-status-success"
        >
          {STATUS_LABELS.done}
        </Badge>
      )
    case 'ended':
      // Unknown outcome (vanished without an exit event): muted, not success.
      return (
        <Badge variant="outline" className="text-muted-foreground">
          {STATUS_LABELS.ended}
        </Badge>
      )
    case 'needs-you':
      return <Badge variant="default">{STATUS_LABELS['needs-you']}</Badge>
    case 'working':
      return <Badge variant="secondary">{STATUS_LABELS.working}</Badge>
  }
}

const PREVIEW_LINES = 6

export function SessionTile({
  session,
  nowMs,
  onFocus
}: {
  session: SessionWithStatus
  nowMs: number
  onFocus: (sessionId: string) => void
}): React.JSX.Element {
  const preview = terminalPlainTextTail(session.ansiTail, PREVIEW_LINES)
  return (
    <Card
      role="button"
      tabIndex={0}
      onClick={() => onFocus(session.sessionId)}
      onKeyDown={(event) => {
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault()
          onFocus(session.sessionId)
        }
      }}
      className={cn(
        'cursor-pointer gap-3 py-4 transition-colors hover:border-ring/60',
        session.status === 'needs-you' && 'border-primary/50'
      )}
    >
      <CardHeader className="px-4">
        <CardTitle className="truncate text-sm" title={session.title}>
          {session.title}
        </CardTitle>
        <div className="flex items-center gap-2">
          <StatusChip status={session.status} />
          <span className="text-xs text-muted-foreground">
            {formatTimeSinceActivity(nowMs, session.lastActivityAt)}
          </span>
        </div>
      </CardHeader>
      <CardContent className="px-4">
        <pre className="h-24 overflow-hidden rounded-md bg-muted/40 p-2 font-mono text-[11px] leading-4 whitespace-pre-wrap text-muted-foreground">
          {preview.length > 0 ? preview.join('\n') : 'No output yet'}
        </pre>
      </CardContent>
    </Card>
  )
}

export function AttentionQueue({
  sessions,
  onFocus
}: {
  sessions: SessionWithStatus[]
  onFocus: (sessionId: string) => void
}): React.JSX.Element | null {
  if (sessions.length === 0) {
    return null
  }
  return (
    <div className="scrollbar-sleek flex items-center gap-2 overflow-x-auto border-b border-border px-4 py-2">
      <span className="shrink-0 text-xs font-medium text-muted-foreground">Needs attention</span>
      {sessions.map((session) => (
        <Button
          key={session.sessionId}
          variant="outline"
          size="sm"
          className="shrink-0 gap-2"
          onClick={() => onFocus(session.sessionId)}
        >
          <span className="max-w-48 truncate">{session.title}</span>
          <StatusChip status={session.status} />
        </Button>
      ))}
    </div>
  )
}
