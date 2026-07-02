import type { AtermHoveredLinkSpan } from './aterm-link-underline-overlay'
import type { AtermMetrics } from './aterm-grid-reflow'

/** How a hovered link activates — picks the affordance-hint wording. `url`/`osc8`
 *  are the engine's URL kinds, `file` is an engine file-path hit, `provider` is an
 *  xterm-style provider link (term_/task_ handles, cwd-resolved paths). */
export type AtermLinkTooltipKind = 'url' | 'osc8' | 'file' | 'provider'

export type AtermLinkTooltipHover = {
  span: AtermHoveredLinkSpan
  /** The link target as shown in the tooltip (URL / raw path / provider text). */
  text: string
  kind: AtermLinkTooltipKind
}

/** PaneManagerOptions.formatLinkTooltip: maps a URL (+ the default affordance
 *  hint) to a richer label (e.g. localhost port worktree labels), possibly async. */
export type AtermFormatLinkTooltip = (
  url: string,
  openLinkHint: string
) => string | null | undefined | Promise<string | null | undefined>

export type AtermLinkTooltip = {
  /** A link is under the pointer; show the tooltip after the hover delay. Safe to
   *  call per hover evaluation — a repeat for the same link text is a no-op. */
  hoverLink: (hover: AtermLinkTooltipHover) => void
  /** The pointer left the link (or the pane state changed); hide immediately. */
  leave: () => void
  dispose: () => void
}

export const ATERM_LINK_TOOLTIP_DELAY_MS = 500

/** The platform-correct modifier affordance for a link kind. Mirrors the wording
 *  of terminal-link-open-hints.ts so aterm tooltips match the rest of orca. */
export function atermLinkTooltipHint(kind: AtermLinkTooltipKind, isMac: boolean): string {
  const mod = isMac ? '⌘' : 'Ctrl'
  const shiftMod = isMac ? '⇧⌘' : 'Shift+Ctrl'
  if (kind === 'file') {
    return `${mod}+click to open or ${shiftMod}+click for default app`
  }
  if (kind === 'provider') {
    // Provider links (term_/task_ handles, resolved paths) activate through the
    // provider's own handler; only the generic modifier affordance is knowable here.
    return `${mod}+click to open`
  }
  return `${mod}+click to open or ${shiftMod}+click for system browser`
}

export type AtermLinkTooltipLabel = {
  /** Shown immediately (the upstream default: "text (modifier hint)"). */
  immediate: string
  /** Resolves to a replacement label, or null to keep `immediate`. Null when the
   *  formatter answered synchronously or does not apply to this link kind. */
  formatted: Promise<string | null> | null
}

/** Decide the tooltip label: the default "text (hint)" now, plus an optional
 *  async replacement from formatLinkTooltip. The formatter only sees URL kinds
 *  (it labels e.g. localhost ports) — matching upstream's WebLinks/OSC8 scope. */
export function resolveAtermLinkTooltipLabel(
  hover: Pick<AtermLinkTooltipHover, 'text' | 'kind'>,
  hint: string,
  formatLinkTooltip: AtermFormatLinkTooltip | undefined
): AtermLinkTooltipLabel {
  const immediate = `${hover.text} (${hint})`
  if (!formatLinkTooltip || (hover.kind !== 'url' && hover.kind !== 'osc8')) {
    return { immediate, formatted: null }
  }
  let result: ReturnType<AtermFormatLinkTooltip>
  try {
    result = formatLinkTooltip(hover.text, hint)
  } catch {
    return { immediate, formatted: null }
  }
  if (result && typeof result === 'object' && 'then' in result) {
    return {
      immediate,
      formatted: Promise.resolve(result).then(
        (next) => next ?? null,
        () => null
      )
    }
  }
  return { immediate: result || immediate, formatted: null }
}

export type AtermLinkTooltipTimeline<T> = {
  hover: (key: string, payload: T) => void
  leave: () => void
  dispose: () => void
}

/** The pure show/hide timing core: show `delayMs` after a hover settles on one
 *  link, keep it steady across repeats for the same key (no flicker while the
 *  pointer moves cell-to-cell along one link), hide instantly on leave/key
 *  change. DOM-free so it is unit-testable with fake timers. */
export function createAtermLinkTooltipTimeline<T>(callbacks: {
  onShow: (payload: T) => void
  onHide: () => void
  delayMs?: number
}): AtermLinkTooltipTimeline<T> {
  const delay = callbacks.delayMs ?? ATERM_LINK_TOOLTIP_DELAY_MS
  let timer: ReturnType<typeof setTimeout> | null = null
  let visible = false
  let key: string | null = null
  let payload: T | null = null

  const hideNow = (): void => {
    if (timer !== null) {
      clearTimeout(timer)
      timer = null
    }
    key = null
    payload = null
    if (visible) {
      visible = false
      callbacks.onHide()
    }
  }

  return {
    hover: (nextKey, nextPayload) => {
      if (nextKey === key) {
        // Same link: keep the visible tooltip or the running delay untouched so
        // moving along the link's span never hides/re-shows (flicker) and never
        // re-fires an async format. Only the payload is refreshed for show-time.
        payload = nextPayload
        return
      }
      hideNow()
      key = nextKey
      payload = nextPayload
      timer = setTimeout(() => {
        timer = null
        visible = true
        callbacks.onShow(payload as T)
      }, delay)
    },
    leave: hideNow,
    dispose: hideNow
  }
}

export type AtermLinkTooltipDeps = {
  canvas: HTMLCanvasElement
  /** The pane's hidden input textarea — keystrokes land there, so its keydown
   *  hides the tooltip (typing scrolls/changes what's under the pointer). */
  textarea: HTMLTextAreaElement
  /** Shared live cell metrics (mutated in place on DPI/font changes) — read at
   *  show time so positioning never goes stale. */
  metrics: AtermMetrics
  isDisposed: () => boolean
  formatLinkTooltip?: AtermFormatLinkTooltip
}

/** The hover tooltip overlay for one aterm pane: a pane-local DOM node (like the
 *  scrollbar overlay) shown near the hovered link span after a short delay, with
 *  the formatLinkTooltip label when it yields one, else "text (modifier hint)".
 *  Main-thread only, so it serves the CPU, GPU, and worker draw paths alike. */
export function createAtermLinkTooltip(deps: AtermLinkTooltipDeps): AtermLinkTooltip {
  const { canvas, textarea, metrics } = deps
  const host = canvas.parentElement
  const isMac = typeof navigator !== 'undefined' && navigator.userAgent.includes('Mac')
  // Monotonic guard: a hide or newer show invalidates in-flight async format
  // results, so a stale label never overwrites the current link's tooltip.
  let showToken = 0

  const element = document.createElement('div')
  element.dataset.testid = 'aterm-link-tooltip' // e2e locator
  Object.assign(element.style, {
    position: 'absolute',
    display: 'none',
    // Above the scrollbar thumb / IME helpers (zIndex 5); non-interactive.
    zIndex: '6',
    pointerEvents: 'none',
    maxWidth: 'calc(100% - 8px)',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
    padding: '3px 8px',
    // STYLEGUIDE floating tier: popover surface + hairline border + the
    // reserved floating shadow; mono because the label is a literal URL/path.
    background: 'var(--popover)',
    color: 'var(--popover-foreground)',
    border: '1px solid var(--border)',
    borderRadius: 'var(--radius-sm, 0.375rem)',
    boxShadow: '0 10px 24px rgb(0 0 0 / 0.18)',
    fontSize: '12px',
    fontFamily: 'var(--font-mono, monospace)'
  } satisfies Partial<CSSStyleDeclaration>)
  host?.appendChild(element)

  // Place the tooltip under the link span's row at its start column (CSS px),
  // clamped into the pane; flip above the row when clipped at the bottom.
  const position = (span: AtermHoveredLinkSpan): void => {
    const cellW = metrics.cellWidth / metrics.dpr
    const cellH = metrics.cellHeight / metrics.dpr
    const hostW = host?.clientWidth ?? canvas.clientWidth
    const hostH = host?.clientHeight ?? canvas.clientHeight
    const left = Math.max(0, Math.min(span.startCol * cellW, hostW - element.offsetWidth))
    let top = (span.row + 1) * cellH + 2
    if (top + element.offsetHeight > hostH) {
      top = Math.max(0, span.row * cellH - element.offsetHeight - 2)
    }
    element.style.left = `${left}px`
    element.style.top = `${top}px`
  }

  const show = (hover: AtermLinkTooltipHover): void => {
    if (deps.isDisposed()) {
      return
    }
    showToken++
    const token = showToken
    const label = resolveAtermLinkTooltipLabel(
      hover,
      atermLinkTooltipHint(hover.kind, isMac),
      deps.formatLinkTooltip
    )
    element.textContent = label.immediate
    element.style.display = ''
    position(hover.span)
    // Async formatter (e.g. localhost labels resolve over IPC): swap the label
    // in only when it beats the next hide/show; reposition for the new width.
    void label.formatted?.then((next) => {
      if (next && token === showToken) {
        element.textContent = next
        position(hover.span)
      }
    })
  }

  const hide = (): void => {
    showToken++
    element.style.display = 'none'
  }

  const timeline = createAtermLinkTooltipTimeline<AtermLinkTooltipHover>({
    onShow: show,
    onHide: hide
  })

  // Anything that changes what's under the pointer hides the tooltip: leaving
  // the canvas, scrolling, pressing a button (click/select), or typing.
  const hideOnInteraction = (): void => timeline.leave()
  canvas.addEventListener('mouseleave', hideOnInteraction)
  canvas.addEventListener('wheel', hideOnInteraction, { passive: true })
  canvas.addEventListener('mousedown', hideOnInteraction)
  textarea.addEventListener('keydown', hideOnInteraction)

  return {
    hoverLink: (hover) => timeline.hover(hover.text, hover),
    leave: () => timeline.leave(),
    dispose: () => {
      canvas.removeEventListener('mouseleave', hideOnInteraction)
      canvas.removeEventListener('wheel', hideOnInteraction)
      canvas.removeEventListener('mousedown', hideOnInteraction)
      textarea.removeEventListener('keydown', hideOnInteraction)
      timeline.dispose()
      element.remove()
    }
  }
}
