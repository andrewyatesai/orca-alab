// Focused view, milestone A: a full-size READ-ONLY text mirror of one session
// (hydration snapshot + live subscriber events, ANSI stripped). No input, no
// kill — the safety default. aterm tile rendering is the named follow-up.
import { useEffect, useRef } from 'react'
import { Button } from '@/components/ui/button'
import { formatTimeSinceActivity } from './session-status'
import { StatusChip, type SessionWithStatus } from './session-tiles'
import { terminalPlainTextTail } from './terminal-text-preview'

const FOCUSED_TAIL_LINES = 600

export function FocusedSessionView({
  session,
  nowMs,
  onBack
}: {
  session: SessionWithStatus
  nowMs: number
  onBack: () => void
}): React.JSX.Element {
  const text = terminalPlainTextTail(session.ansiTail, FOCUSED_TAIL_LINES).join('\n')
  const scrollRef = useRef<HTMLDivElement | null>(null)
  useEffect(() => {
    // Pin to the newest output, like a live terminal tail.
    const el = scrollRef.current
    if (el) {
      el.scrollTop = el.scrollHeight
    }
  }, [text])
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex items-center gap-3 border-b border-border px-4 py-2">
        <Button variant="ghost" size="sm" onClick={onBack}>
          ← All sessions
        </Button>
        <span className="truncate text-sm font-medium" title={session.title}>
          {session.title}
        </span>
        <StatusChip status={session.status} />
        <span className="text-xs text-muted-foreground">
          {formatTimeSinceActivity(nowMs, session.lastActivityAt)}
        </span>
        <span className="ml-auto text-xs text-muted-foreground">Read-only view</span>
      </div>
      <div ref={scrollRef} className="scrollbar-sleek min-h-0 flex-1 overflow-y-auto p-4">
        <pre className="font-mono text-xs leading-5 whitespace-pre-wrap text-foreground">
          {text || 'No output yet'}
        </pre>
      </div>
    </div>
  )
}
