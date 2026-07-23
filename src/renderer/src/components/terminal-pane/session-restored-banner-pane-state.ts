import type { ManagedPane } from '@/lib/pane-manager/pane-manager'

export type SessionRestoredBannerPane = Pick<ManagedPane, 'id' | 'container' | 'terminal'>

export type SessionRestoredBannerStartup =
  | {
      showSessionRestoredBanner?: boolean
    }
  | null
  | undefined

/** Per-pane banner payload: lastCommand powers the re-run affordance (#7596). */
export type SessionRestoredBannerState = {
  lastCommand: string | null
}

export type SessionRestoredBannerDismissEvent = KeyboardEvent | PointerEvent

// Why: pointerdown on this element must activate the affordance, not dismiss
// the banner out from under its own click (the dismiss listener is capture-phase).
export const SESSION_RESTORED_BANNER_ACTION_ATTRIBUTE = 'data-session-restored-banner-action'

/** #7596 offer gate: single-line, ≤200 chars — the banner ellipsis is not a shell. */
export function offerableRestoredLastCommand(lastCommand: string | null | undefined): string | null {
  if (typeof lastCommand !== 'string') {
    return null
  }
  const trimmed = lastCommand.trim()
  if (!trimmed || trimmed.length > 200 || /[\r\n]/.test(trimmed)) {
    return null
  }
  return trimmed
}

export function addSessionRestoredBannerPane(
  states: ReadonlyMap<number, SessionRestoredBannerState>,
  paneId: number,
  lastCommand: string | null = null
): Map<number, SessionRestoredBannerState> {
  const existing = states.get(paneId)
  // Why: an agent-resume trigger (null) must not clobber a lastCommand already
  // recorded for the pane; identical state returns the same reference.
  if (existing && (existing.lastCommand === lastCommand || lastCommand === null)) {
    return states instanceof Map ? states : new Map(states)
  }
  return new Map(states).set(paneId, { lastCommand })
}

export function removeSessionRestoredBannerPane(
  states: ReadonlyMap<number, SessionRestoredBannerState>,
  paneId: number
): Map<number, SessionRestoredBannerState> {
  if (!states.has(paneId)) {
    return states instanceof Map ? states : new Map(states)
  }
  const next = new Map(states)
  next.delete(paneId)
  return next
}

export function pruneSessionRestoredBannerPanes(
  states: ReadonlyMap<number, SessionRestoredBannerState>,
  panes: readonly SessionRestoredBannerPane[]
): Map<number, SessionRestoredBannerState> {
  const livePaneIds = new Set(panes.map((pane) => pane.id))
  if ([...states.keys()].every((paneId) => livePaneIds.has(paneId))) {
    return states instanceof Map ? states : new Map(states)
  }
  return new Map([...states].filter(([paneId]) => livePaneIds.has(paneId)))
}

export function getSessionRestoredBannerDismissPaneId(
  event: SessionRestoredBannerDismissEvent,
  panes: readonly SessionRestoredBannerPane[]
): number | null {
  const targetElement =
    event.target instanceof Element
      ? event.target
      : event.target instanceof Node
        ? event.target.parentElement
        : null
  const paneElement = targetElement?.closest('.pane[data-leaf-id]')
  if (!paneElement) {
    return null
  }
  return panes.find((pane) => pane.container === paneElement)?.id ?? null
}

export function dismissSessionRestoredBannerPanes(
  states: ReadonlyMap<number, SessionRestoredBannerState>,
  event: SessionRestoredBannerDismissEvent,
  panes: readonly SessionRestoredBannerPane[]
): Map<number, SessionRestoredBannerState> {
  const targetElement = event.target instanceof Element ? event.target : null
  if (targetElement?.closest(`[${SESSION_RESTORED_BANNER_ACTION_ATTRIBUTE}]`)) {
    return states instanceof Map ? states : new Map(states)
  }
  const paneId = getSessionRestoredBannerDismissPaneId(event, panes)
  if (paneId === null) {
    return new Map()
  }
  return removeSessionRestoredBannerPane(states, paneId)
}

export function seedStartupSessionRestoredBanner(
  startup: SessionRestoredBannerStartup,
  paneId: number,
  onShowSessionRestoredBanner: (paneId: number) => void
): void {
  if (startup?.showSessionRestoredBanner === true) {
    onShowSessionRestoredBanner(paneId)
  }
}

export function syncSessionRestoredBannerTitleSpace(args: {
  panes: readonly SessionRestoredBannerPane[]
  paneTitles: Readonly<Record<number, string>>
  renamingPaneId: number | null
  sessionRestoredBannerPanes: ReadonlyMap<number, SessionRestoredBannerState>
}): boolean {
  let needsFit = false
  for (const pane of args.panes) {
    const shouldShow =
      !!args.paneTitles[pane.id] ||
      args.renamingPaneId === pane.id ||
      args.sessionRestoredBannerPanes.has(pane.id)
    const hadTitle = pane.container.hasAttribute('data-has-title')
    if (shouldShow && !hadTitle) {
      pane.container.setAttribute('data-has-title', '')
      needsFit = true
    } else if (!shouldShow && hadTitle) {
      pane.container.removeAttribute('data-has-title')
      needsFit = true
    }
  }
  return needsFit
}
