// Context-menu link/path target resolution + opening (#9279 / CM-A2). Split from
// aterm-link-input so the hover/click wiring stays under the line cap; the deps
// are the SAME closures the click path uses, so menu opens can't drift from it.

import type { AtermTerminal } from './aterm_wasm.js'
import { resolveProviderLinkAt, type AtermLinkProviderSource } from './aterm-provider-link-hit'
import type {
  AtermFileLinkOpener,
  AtermLinkDeps,
  AtermLinkOpener,
  AtermOscLinkOpener
} from './aterm-link-input'

// Map a pointer position to a (col, display-row) grid cell. Identical mapping to
// aterm-selection-input.ts: clientX/Y minus the canvas rect (not offsetX/Y) so
// synthetic e2e events and real events agree, scaled to device pixels; the row
// is already display-offset-inclusive. Takes a bare point (not a MouseEvent) so
// the context-menu resolver can pass right-click coordinates.
export function atermLinkPointToCell(
  point: { clientX: number; clientY: number },
  deps: Pick<AtermLinkDeps, 'canvas' | 'metrics' | 'getChrome'>
): { col: number; row: number } {
  const rect = deps.canvas.getBoundingClientRect()
  // Effects chrome shifts the canvas rect up-left of the grid (negative margins);
  // subtract the grid's in-frame offset so link hit-testing stays grid-relative.
  const chrome = deps.getChrome?.() ?? { pad: 0, head: 0 }
  const deviceX = (point.clientX - rect.left) * deps.metrics.dpr - chrome.pad
  const deviceY = (point.clientY - rect.top) * deps.metrics.dpr - chrome.pad - chrome.head
  const col = Math.max(0, Math.floor(deviceX / deps.metrics.cellWidth))
  const row = Math.max(0, Math.floor(deviceY / deps.metrics.cellHeight))
  return { col, row }
}

/** A link/path target resolved at a screen point for the context menu (#9279):
 *  engine kinds map to url/osc8/file; provider links carry their own activate. */
export type AtermContextLinkTarget =
  | { kind: 'url' | 'osc8'; url: string }
  | { kind: 'file'; rawPathText: string }
  | { kind: 'provider'; text: string; activate: (event: MouseEvent) => void }

// Link kinds from the wasm engine: 0=osc8, 1=url, 2=file_path, 3=other.
const LINK_KIND_OSC8 = 0
const LINK_KIND_URL = 1
const LINK_KIND_FILE_PATH = 2

/** The click path's own closures, threaded by attachAtermLinkInput. */
export type AtermContextTargetDeps = {
  term: AtermTerminal
  isDisposed: () => boolean
  openUrl: AtermLinkOpener
  openOscUrl: AtermOscLinkOpener
  getFileLinkOpener: () => AtermFileLinkOpener | null
  getLinkProviders?: AtermLinkProviderSource
  /** Worker facade's fresh hit query; undefined in-process (sync link_at is live). */
  asyncLinkAt?: (
    row: number,
    col: number
  ) => Promise<{ url: string; kind: number } | null | undefined>
  pointToCell: (point: { clientX: number; clientY: number }) => { col: number; row: number }
  /** 1-based ABSOLUTE buffer line of a display row (provider hit-test space). */
  absoluteLineFor: (row: number) => number
}

/** Resolve the target under a client point: mirrors onClick's resolution order
 *  (alt-screen/tracking → engine hit, fresh on the worker path → provider
 *  fallback) but returns the target instead of activating it. */
export async function resolveAtermContextLinkTarget(
  deps: AtermContextTargetDeps,
  clientX: number,
  clientY: number
): Promise<AtermContextLinkTarget | null> {
  const { term, isDisposed } = deps
  // Mouse tracking (menu open carries no Shift bypass) and alt-screen: the app
  // owns the pointer — same defer as the click path.
  if (isDisposed() || term.is_alt_screen || term.is_mouse_tracking) {
    return null
  }
  const { col, row } = deps.pointToCell({ clientX, clientY })
  const hit = deps.asyncLinkAt ? await deps.asyncLinkAt(row, col) : term.link_at(row, col)
  if (isDisposed()) {
    return null
  }
  if (hit) {
    if (hit.kind === LINK_KIND_OSC8) {
      return { kind: 'osc8', url: hit.url }
    }
    if (hit.kind === LINK_KIND_URL) {
      return { kind: 'url', url: hit.url }
    }
    // Kind 3 "other": nothing can open it — no target, matching the tooltip.
    return hit.kind === LINK_KIND_FILE_PATH ? { kind: 'file', rawPathText: hit.url } : null
  }
  const providers = deps.getLinkProviders?.() ?? []
  if (providers.length === 0) {
    return null
  }
  const link = await resolveProviderLinkAt(providers, deps.absoluteLineFor(row), col + 1)
  if (!link || isDisposed()) {
    return null
  }
  return { kind: 'provider', text: link.text, activate: (event) => link.activate(event, link.text) }
}

/** Open a previously resolved target through the SAME routing the modifier-click
 *  path uses (in-app preference, scheme-aware OSC-8, late-bound file opener,
 *  provider activate). */
export function openAtermContextLinkTarget(
  deps: AtermContextTargetDeps,
  target: AtermContextLinkTarget,
  opts: { openWithSystemDefault: boolean }
): void {
  if (deps.isDisposed()) {
    return
  }
  if (target.kind === 'file') {
    deps.getFileLinkOpener()?.(target.rawPathText, opts.openWithSystemDefault)
    return
  }
  if (target.kind === 'provider') {
    // Provider activates re-check the platform activation modifier (Cmd/Ctrl), so
    // the synthesized menu event must carry it or the activate is a silent no-op.
    const isMac = typeof navigator !== 'undefined' && navigator.userAgent.includes('Mac')
    target.activate(new MouseEvent('click', { metaKey: isMac, ctrlKey: !isMac }))
    return
  }
  if (target.kind === 'url') {
    deps.openUrl(target.url, { forceSystemBrowser: opts.openWithSystemDefault })
    return
  }
  // OSC-8: the scheme router reads modifiers off the event; Shift = system-default hatch.
  deps.openOscUrl(target.url, new MouseEvent('click', { shiftKey: opts.openWithSystemDefault }))
}
