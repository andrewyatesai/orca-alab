import React from 'react'
import { AlertTriangle } from 'lucide-react'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { useAppStore } from '../../store'
import { useDaemonRuntimeStatus } from '@/lib/daemon-runtime-status-store'
import { getDaemonStatusIndicatorCopy } from '../daemon-status/daemon-status-copy'

/**
 * Low-key persistent indicator for a degraded/failed terminal daemon
 * (docs/reference/daemon-staleness-ux.md §Phase 2). Renders nothing while the daemon is
 * running or starting; clicking opens Settings → Terminal, where the guarded
 * Manage Sessions restart flow lives.
 */
export function DaemonStatusSegment({
  compact,
  iconOnly
}: {
  compact: boolean
  iconOnly: boolean
}): React.JSX.Element | null {
  const status = useDaemonRuntimeStatus()
  const setActiveView = useAppStore((s) => s.setActiveView)
  const openSettingsTarget = useAppStore((s) => s.openSettingsTarget)

  const copy = getDaemonStatusIndicatorCopy(status)
  if (!copy) {
    return null
  }

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          className="inline-flex items-center gap-1.5 cursor-pointer rounded px-1 py-0.5 hover:bg-accent/70"
          aria-label={copy.ariaLabel}
          onClick={() => {
            openSettingsTarget({ pane: 'terminal', repoId: null })
            setActiveView('settings')
          }}
        >
          <AlertTriangle className="size-3 text-destructive" />
          {!compact && !iconOnly && (
            <span className="text-[11px] text-muted-foreground">{copy.label}</span>
          )}
        </button>
      </TooltipTrigger>
      <TooltipContent side="top" sideOffset={6} className="max-w-[280px]">
        <div>{copy.tooltip}</div>
        {status.detail ? (
          <div className="mt-1 break-words text-muted-foreground">{status.detail}</div>
        ) : null}
      </TooltipContent>
    </Tooltip>
  )
}
