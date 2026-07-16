// Focused view: a full-size READ-ONLY aterm tile of one session (hydration
// snapshot + live subscriber bytes into an in-process CPU engine). The
// milestone-A text mirror stays as the loading overlay and the automatic
// fallback if the engine fails — the coordinator never shows a blank pane.
// No input, no kill — the safety default.
import { useEffect, useRef, useState } from 'react'
import { Button } from '@/components/ui/button'
import { FocusedSessionTerminal } from './focused-session-terminal'
import { formatTimeSinceActivity } from './session-status'
import { StatusChip, type SessionWithStatus } from './session-tiles'
import { terminalPlainTextTail } from './terminal-text-preview'

const FOCUSED_TAIL_LINES = 600

type EngineState = 'loading' | 'ready' | 'failed'

function TextMirror({ ansiTail }: { ansiTail: string }): React.JSX.Element {
  const text = terminalPlainTextTail(ansiTail, FOCUSED_TAIL_LINES).join('\n')
  const scrollRef = useRef<HTMLDivElement | null>(null)
  useEffect(() => {
    // Pin to the newest output, like a live terminal tail.
    const el = scrollRef.current
    if (el) {
      el.scrollTop = el.scrollHeight
    }
  }, [text])
  return (
    <div
      ref={scrollRef}
      className="scrollbar-sleek absolute inset-0 overflow-y-auto bg-background p-4"
    >
      <pre className="font-mono text-xs leading-5 whitespace-pre-wrap text-foreground">
        {text || 'No output yet'}
      </pre>
    </div>
  )
}

export function FocusedSessionView({
  session,
  nowMs,
  onBack
}: {
  session: SessionWithStatus
  nowMs: number
  onBack: () => void
}): React.JSX.Element {
  const [engine, setEngine] = useState<EngineState>('loading')
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
      <div className="relative min-h-0 flex-1">
        {engine !== 'failed' && (
          <FocusedSessionTerminal
            sessionId={session.sessionId}
            onReady={() => setEngine('ready')}
            onFailed={() => setEngine('failed')}
          />
        )}
        {/* Mounted after the terminal so it stacks above it while loading. */}
        {engine !== 'ready' && <TextMirror ansiTail={session.ansiTail} />}
      </div>
    </div>
  )
}
