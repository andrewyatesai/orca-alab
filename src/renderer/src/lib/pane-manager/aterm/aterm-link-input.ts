import type { AtermTerminal } from './aterm_wasm.js'
import { shouldForwardMouse } from './aterm-mouse-input'
import type { AtermMetrics } from './aterm-grid-reflow'
import type { AtermHoveredLinkSpan } from './aterm-link-underline-overlay'
import type { AtermWorkerAsyncFacade } from './aterm-worker-query-channel'
import type { ILink } from './terminal-types'
import {
  providerRangeContainsCell,
  resolveProviderLinkAt,
  type AtermLinkProviderSource
} from './aterm-provider-link-hit'
import type { AtermLinkTooltip, AtermLinkTooltipKind } from './aterm-link-tooltip'
import {
  atermLinkPointToCell,
  openAtermContextLinkTarget,
  resolveAtermContextLinkTarget,
  type AtermContextLinkTarget,
  type AtermContextTargetDeps
} from './aterm-context-link-target'

export type { AtermLinkProviderSource } from './aterm-provider-link-hit'
export type { AtermContextLinkTarget } from './aterm-context-link-target'

/** Opens a detected link target; the controller threads orca's URL opener here
 *  (forceSystemBrowser mirrors xterm's Shift+modifier "open in system browser"
 *  escape hatch). */
export type AtermLinkOpener = (url: string, opts: { forceSystemBrowser: boolean }) => void

/** Opens an OSC-8 (kind 0) hyperlink target. Receives the raw MouseEvent so the
 *  scheme-aware router can read the activation modifier + Shift system-default
 *  hatch itself (mirrors the xterm linkHandler.activate signature). */
export type AtermOscLinkOpener = (url: string, event: MouseEvent) => void

/** Opens a detected file-path link (kind 2). `rawPathText` is the matched span
 *  exactly as it appeared on the row; the closure resolves it against the pane's
 *  cwd/runtime and opens it. `openWithSystemDefault` mirrors xterm's Shift hatch. */
export type AtermFileLinkOpener = (rawPathText: string, openWithSystemDefault: boolean) => void

export type AtermLinkDeps = {
  canvas: HTMLCanvasElement
  term: AtermTerminal
  /** Shared live cell metrics (mutated in place by the grid reflow on DPI/font
   *  changes) — read per event so link hit-testing never goes stale. */
  metrics: AtermMetrics
  redraw: () => void
  isDisposed: () => boolean
  openUrl: AtermLinkOpener
  /** OSC-8 (kind 0) targets carry arbitrary schemes (file://, Windows paths) —
   *  routed scheme-aware, NOT through the http(s)-only URL opener (#6880). */
  openOscUrl: AtermOscLinkOpener
  /** Latest file-path opener (kind 2), late-bound by the controller. Null until
   *  the pane's cwd/runtime context is threaded in; then kind-2 clicks open. */
  getFileLinkOpener: () => AtermFileLinkOpener | null
  /** Latest xterm-style link providers (file paths, term_/task_ handles), late-
   *  bound by the facade. They run only where the engine reports no link, and
   *  read line text through the facade buffer (viewport rows — the same row-text
   *  source the a11y mirror reads, so the worker path serves them too). */
  getLinkProviders?: AtermLinkProviderSource
  /** Hover tooltip sink (main-thread DOM overlay, see aterm-link-tooltip.ts).
   *  Optional so tests exercising only hit-testing/cursor logic can omit it. */
  linkTooltip?: Pick<AtermLinkTooltip, 'hoverLink' | 'leave'>
  /** Live window-space chrome offsets (device px) when the worker frame carries
   *  effects chrome; undefined/0 in-process (the canvas rect IS the grid). */
  getChrome?: () => { pad: number; head: number }
}

export type AtermLinkInput = {
  /** The display-row cell span of the link currently under the pointer, or null.
   *  The draw paths read this each frame to paint the hover underline; it's
   *  cleared whenever the pointer leaves the link / a non-link cell / alt-screen. */
  hoveredSpan: () => AtermHoveredLinkSpan | null
  /** Invalidate the last-hovered-cell cache so the NEXT mousemove re-evaluates
   *  links even when the pointer returns to the same cell (reveal recovery —
   *  buffer content may have changed while the pane was hidden). */
  resetHoverCache: () => void
  /** Resolve the link/path target under a client point for the context menu:
   *  engine hit first (fresh worker query when available), provider fallback;
   *  null on alt-screen / mouse tracking (the app owns the pointer there). */
  contextLinkTargetAt: (clientX: number, clientY: number) => Promise<AtermContextLinkTarget | null>
  /** Open a previously resolved context target through the SAME routing the
   *  modifier-click path uses (in-app preference, scheme-aware OSC-8, late-bound
   *  file opener, provider activate). */
  openContextTarget: (
    target: AtermContextLinkTarget,
    opts: { openWithSystemDefault: boolean }
  ) => void
  dispose: () => void
}

// Link kinds from the wasm engine: 0=osc8, 1=url, 2=file_path, 3=other.
const LINK_KIND_OSC8 = 0
const LINK_KIND_URL = 1
const LINK_KIND_FILE_PATH = 2

// Engine kind → tooltip affordance wording; null for kinds nothing can open
// (kind 3 "other"), where showing a click hint would lie.
function tooltipKindForEngineLink(kind: number): AtermLinkTooltipKind | null {
  if (kind === LINK_KIND_OSC8) {
    return 'osc8'
  }
  if (kind === LINK_KIND_URL) {
    return 'url'
  }
  return kind === LINK_KIND_FILE_PATH ? 'file' : null
}

// Two hovered link spans are equal when they cover the same cells; used to avoid
// redrawing the underline while the pointer moves within one link span.
function spansEqual(a: AtermHoveredLinkSpan | null, b: AtermHoveredLinkSpan | null): boolean {
  if (a === null || b === null) {
    return a === b
  }
  return a.row === b.row && a.startCol === b.startCol && a.endCol === b.endCol
}

// Platform link-activation modifier: Cmd on macOS, Ctrl elsewhere. Mirrors
// terminal-link-handlers.isTerminalLinkActivation so the aterm path matches the
// default terminal's "modifier+click opens the link" convention.
function isLinkActivation(event: MouseEvent): boolean {
  const isMac = typeof navigator !== 'undefined' && navigator.userAgent.includes('Mac')
  return isMac ? event.metaKey : event.ctrlKey
}

/** Wire hover + modifier-click link activation on the aterm canvas. Mirrors
 *  attachAtermSelectionInput's structure; the wasm engine does the link
 *  detection via link_at, and we only paint a pointer cursor + open URLs. */
export function attachAtermLinkInput(deps: AtermLinkDeps): AtermLinkInput {
  const { canvas, term, redraw, isDisposed, openUrl, openOscUrl, getFileLinkOpener, getLinkProviders } =
    deps
  const { linkTooltip } = deps
  // Worker-backed term: link_at returns the lagging snapshot and the loader drives the
  // canvas cursor from the worker's hoverCursor each STATE. Detect the async capability
  // to resolve fresh hits on click + clear the worker hover, and stop fighting the
  // loader's cursor. In-process exposes neither → the synchronous path below is
  // byte-identical.
  const workerTerm = term as AtermTerminal & Partial<AtermWorkerAsyncFacade>
  const asyncLinkAt = workerTerm.linkAtAsync
  const clearWorkerHover = workerTerm.clearHover
  let moveScheduled = false
  let lastCol = -1
  let lastRow = -1
  let pendingEvent: MouseEvent | null = null
  // Tracked so dispose() can cancel a pending hover frame (cleared when it fires).
  let hoverRafId: number | null = null
  // The link span under the pointer (display-row cells); the draw paths read it to
  // paint the hover underline. Cleared whenever the cursor affordance is cleared,
  // and a redraw is requested only when it actually changes so the underline
  // appears/disappears without per-pixel repaints.
  let hovered: AtermHoveredLinkSpan | null = null
  // The provider link under the pointer (engine reported no link there). Cached
  // so the click can activate + preventDefault synchronously, and so hover/leave
  // fire once per link instead of per cell.
  let providerHover: { link: ILink; row: number; startCol: number; endCol: number } | null = null
  // Monotonic guard: any hover change invalidates in-flight provider queries.
  let providerSeq = 0

  // 1-based ABSOLUTE buffer line of a display row (what providers hit-test with).
  const absoluteLineFor = (row: number): number => term.display_origin_absolute + row + 1

  // This row's cell segment of a (possibly wrapped multi-row) provider link.
  const providerLinkSpanFor = (link: ILink, row: number): AtermHoveredLinkSpan => {
    const line = absoluteLineFor(row)
    // canvas.width is the FRAME (chrome-padded when effects chrome is on) — take
    // the grid-only width so a wrapped span's end column isn't over-counted.
    const gridWidth = canvas.width - 2 * (deps.getChrome?.().pad ?? 0)
    const gridCols = Math.max(1, Math.round(gridWidth / deps.metrics.cellWidth))
    return {
      row,
      // range is 1-based inclusive; the span is 0-based with an exclusive end.
      startCol: line === link.range.start.y ? link.range.start.x - 1 : 0,
      endCol: line === link.range.end.y ? link.range.end.x : gridCols
    }
  }

  const leaveProviderLink = (event: MouseEvent | null): void => {
    providerSeq++
    if (!providerHover) {
      return
    }
    const { link } = providerHover
    providerHover = null
    link.leave?.(event ?? new MouseEvent('mouseleave'), link.text)
    // Written directly on BOTH paths: the worker loader only writes the canvas
    // cursor when the engine's hoverCursor CHANGES, and the engine never saw
    // this provider link, so nothing else clears the pointer affordance.
    canvas.style.cursor = ''
  }

  // Engine reported no link here: ask the registered providers (async — they
  // probe path existence / read wrapped lines). Results are dropped when the
  // pointer moved or a newer query started.
  const queryProviders = (event: MouseEvent, col: number, row: number): void => {
    leaveProviderLink(event)
    const providers = getLinkProviders?.() ?? []
    if (providers.length === 0) {
      return
    }
    const seq = providerSeq
    void resolveProviderLinkAt(providers, absoluteLineFor(row), col + 1).then((link) => {
      if (isDisposed() || seq !== providerSeq || col !== lastCol || row !== lastRow || !link) {
        return
      }
      const span = providerLinkSpanFor(link, row)
      providerHover = { link, row, startCol: span.startCol, endCol: span.endCol }
      canvas.style.cursor = 'pointer'
      link.hover?.(event, link.text)
      linkTooltip?.hoverLink({ span, text: link.text, kind: 'provider' })
      if (!spansEqual(hovered, span)) {
        hovered = span
        redraw()
      }
    })
  }

  // Drop the link affordance (pointer cursor + underline). Requests a redraw only
  // when an underline was actually showing, so a non-link move stays cheap.
  const clearCursor = (): void => {
    // Worker path: clear the worker's hover (→ '' next STATE; the loader applies the
    // cursor) instead of writing the canvas cursor here, which that STATE would
    // overwrite. In-process: clear the cursor directly (byte-identical).
    if (clearWorkerHover) {
      clearWorkerHover()
    } else {
      canvas.style.cursor = ''
    }
    if (hovered) {
      hovered = null
      redraw()
    }
  }

  // Throttle hover hit-testing to one rAF frame, and skip re-querying when the
  // pointer is still on the same cell (mousemove fires per pixel).
  const evaluateHover = (): void => {
    moveScheduled = false
    hoverRafId = null
    const event = pendingEvent
    pendingEvent = null
    if (!event || isDisposed()) {
      return
    }
    // On the alternate screen TUIs own the mouse; never show a link cursor.
    // Likewise when mouse tracking is on (no Shift): the app owns the pointer,
    // so don't show a link cursor — the forwarder is reporting motion to it.
    if (term.is_alt_screen || shouldForwardMouse(term, event)) {
      leaveProviderLink(event)
      linkTooltip?.leave()
      clearCursor()
      return
    }
    const { col, row } = atermLinkPointToCell(event, deps)
    if (col === lastCol && row === lastRow) {
      return
    }
    lastCol = col
    lastRow = row
    // NOTE: the wasm signature is link_at(row, col) — match the .d.ts order. The call
    // still posts the hover position the worker needs to compute its underline + cursor.
    const hit = term.link_at(row, col)
    // Track the hovered span so the draw paths underline it; redraw only when the
    // span actually changes (entering/leaving a link, or moving to a different
    // link span) — moving within the same link span is a no-op.
    let nextSpan: AtermHoveredLinkSpan | null = null
    if (hit) {
      leaveProviderLink(event)
      // In-process answers link_at synchronously → set the cursor here. Worker-backed:
      // link_at lags a frame and the loader drives the cursor from hoverCursor, so don't
      // overwrite it with a stale value.
      if (!asyncLinkAt) {
        canvas.style.cursor = 'pointer'
      }
      nextSpan = { row, startCol: hit.start_col, endCol: hit.end_col }
      // Tooltip is keyed by link text, so per-cell repeats inside one span are
      // no-ops in its timeline (no flicker, no re-format).
      const tooltipKind = tooltipKindForEngineLink(hit.kind)
      if (tooltipKind) {
        linkTooltip?.hoverLink({ span: nextSpan, text: hit.url, kind: tooltipKind })
      } else {
        linkTooltip?.leave()
      }
    } else if (
      providerHover &&
      providerRangeContainsCell(providerHover.link.range, col + 1, absoluteLineFor(row))
    ) {
      // Still inside the same provider link (possibly on another wrapped row):
      // keep the hover alive without re-querying; just move the underline segment.
      const span = providerLinkSpanFor(providerHover.link, row)
      providerHover = { link: providerHover.link, ...span }
      nextSpan = span
      // Same text key → the timeline keeps the tooltip steady across the
      // wrapped link's rows instead of hiding/re-delaying.
      linkTooltip?.hoverLink({ span, text: providerHover.link.text, kind: 'provider' })
    } else {
      if (!asyncLinkAt) {
        canvas.style.cursor = ''
      }
      // No link here: hide/cancel now; a provider resolution re-hovers async.
      linkTooltip?.leave()
      queryProviders(event, col, row)
    }
    if (!spansEqual(hovered, nextSpan)) {
      hovered = nextSpan
      redraw()
    }
  }

  const onMouseMove = (event: MouseEvent): void => {
    if (isDisposed()) {
      return
    }
    pendingEvent = event
    if (moveScheduled) {
      return
    }
    moveScheduled = true
    hoverRafId = requestAnimationFrame(evaluateHover)
  }

  // Open a resolved link hit (OSC-8 via the scheme router, URL via openUrl,
  // file-path via the late-bound opener).
  const openHit = (hit: { url: string; kind: number }, event: MouseEvent): void => {
    // Why: OSC-8 targets are app-minted URIs (file://, C:\ paths) — the http(s)-only
    // URL opener would hand them to the browser instead of orca's file routing (#6880).
    if (hit.kind === LINK_KIND_OSC8) {
      event.preventDefault()
      openOscUrl(hit.url, event)
      return
    }
    if (hit.kind === LINK_KIND_URL) {
      event.preventDefault()
      openUrl(hit.url, { forceSystemBrowser: event.shiftKey })
      return
    }
    // File paths: defer to the late-bound opener (resolves cwd/runtime + opens).
    // Null until the pane's context is threaded in → no-op, never a crash.
    if (hit.kind === LINK_KIND_FILE_PATH) {
      const openFileLink = getFileLinkOpener()
      if (!openFileLink) {
        return
      }
      event.preventDefault()
      openFileLink(hit.url, event.shiftKey)
    }
  }

  const onClick = (event: MouseEvent): void => {
    if (isDisposed() || event.button !== 0 || !isLinkActivation(event)) {
      return
    }
    // Mouse tracking on (no Shift) → the click is a report to the app, not a
    // link activation; defer just like the alternate-screen case.
    if (term.is_alt_screen || shouldForwardMouse(term, event)) {
      return
    }
    const { col, row } = atermLinkPointToCell(event, deps)
    // preventDefault must land in THIS task (it is inert after an await); the
    // last hover hit already tells us synchronously that a link is here.
    if (hovered && hovered.row === row && col >= hovered.startCol && col < hovered.endCol) {
      event.preventDefault()
    }
    // Provider link (the engine reported none at this cell): activate from the
    // cached hover — the provider's activate re-checks the platform modifier.
    if (
      providerHover &&
      providerRangeContainsCell(providerHover.link.range, col + 1, absoluteLineFor(row))
    ) {
      providerHover.link.activate(event, providerHover.link.text)
      return
    }
    // Worker-backed term: the sync link_at snapshot lags, so resolve the real hit via the
    // async query channel; in-process answers synchronously.
    if (asyncLinkAt) {
      void asyncLinkAt(row, col).then((hit) => {
        if (!isDisposed() && hit) {
          openHit(hit, event)
        }
      })
      return
    }
    const hit = term.link_at(row, col)
    if (!hit) {
      return
    }
    openHit(hit, event)
  }

  // Context-menu target resolution/opening (#9279): the SAME closures the click
  // path uses, so menu targets can't drift from modifier-click behavior.
  const contextTargetDeps: AtermContextTargetDeps = {
    term,
    isDisposed,
    openUrl,
    openOscUrl,
    getFileLinkOpener,
    getLinkProviders,
    asyncLinkAt,
    pointToCell: (point) => atermLinkPointToCell(point, deps),
    absoluteLineFor
  }

  canvas.addEventListener('mousemove', onMouseMove)
  canvas.addEventListener('click', onClick)

  return {
    hoveredSpan: () => hovered,
    contextLinkTargetAt: (clientX, clientY) =>
      resolveAtermContextLinkTarget(contextTargetDeps, clientX, clientY),
    openContextTarget: (target, opts) =>
      openAtermContextLinkTarget(contextTargetDeps, target, opts),
    resetHoverCache: () => {
      // Only the same-cell short-circuit is dropped; the current hover/underline
      // stays until the next mousemove re-evaluates it (mirrors upstream #9061).
      lastCol = -1
      lastRow = -1
    },
    dispose: () => {
      // Cancel a queued hover frame so evaluateHover can't run after teardown.
      if (hoverRafId !== null) {
        cancelAnimationFrame(hoverRafId)
        hoverRafId = null
      }
      canvas.removeEventListener('mousemove', onMouseMove)
      canvas.removeEventListener('click', onClick)
      // Fire the provider's leave (hides its tooltip) and drop in-flight queries.
      leaveProviderLink(null)
      linkTooltip?.leave()
      clearCursor()
    }
  }
}
