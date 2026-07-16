// Wire contract for the cross-pane spill compositor in the SHARED render worker
// (stage 4). Split from aterm-render-worker-protocol like the font protocol to
// keep that file inside the line budget. Spill state is WORKER-GLOBAL (one
// overlay canvas composites every worker pane), but the messages travel as
// pane-stamped commands: the manager only exposes per-pane posts, and
// `spillPaneRects` NEEDS the stamped paneId — it binds the sending pane's
// engine (the spill byte source) to its overlay paneKey. The worker entry
// routes the whole family to the compositor BEFORE the per-pane dispatch, so a
// canvas transfer can never be dropped by a racing pane dispose.
//
// Types-only, like the parent protocol file.

import type { SpillPaneGeometry } from './aterm-spill-pane-scratch'

/** Device-px backing size of the overlay canvas (matches SpillOverlayBox). */
export type AtermWorkerSpillBox = { widthPx: number; heightPx: number }

/** Adopt a freshly transferred overlay canvas. `epoch` is monotone across
 *  canvas generations (worker respawn / release-rebind ships a fresh element +
 *  a higher epoch); the worker drops inits at or below its adopted epoch, so a
 *  late message addressed to a retired canvas can never regress the surface. */
export type AtermWorkerSpillCanvasInit = {
  type: 'spillCanvasInit'
  epoch: number
  canvas: OffscreenCanvas
  box: AtermWorkerSpillBox
  /** Informational (geometry is already integer device px): diagnostics only. */
  dpr: number
}

/** Resize the overlay backing (container resize/zoom). Implicitly clears; the
 *  worker recomposites every pane from its retained scratch. */
export type AtermWorkerSpillOverlayBox = {
  type: 'spillOverlayBox'
  epoch: number
  box: AtermWorkerSpillBox
}

/** Coalesced geometry push for the SENDING pane (change-fed, never per-frame):
 *  binds the stamped paneId's engine to `paneKey` and adopts the measured
 *  frameOrigin/clipRect/stripRects/outsideRects. */
export type AtermWorkerSpillPaneRects = {
  type: 'spillPaneRects'
  paneKey: string
  geometry: SpillPaneGeometry
}

/** Drop a pane from the worker compositor and clear its strips once (chrome
 *  returned to 0/0, or the pane left the overlay registry). */
export type AtermWorkerSpillUnregister = { type: 'spillUnregister'; paneKey: string }

/** Release the overlay canvas (last worker spill pane unbound): clear it and
 *  drop the reference so the retired element can be garbage collected. */
export type AtermWorkerSpillRelease = { type: 'spillRelease'; epoch: number }

export type AtermWorkerSpillCommand =
  | AtermWorkerSpillCanvasInit
  | AtermWorkerSpillOverlayBox
  | AtermWorkerSpillPaneRects
  | AtermWorkerSpillUnregister
  | AtermWorkerSpillRelease

/** Narrow any wire message to the spill family (the entry's pre-dispatch route). */
export function isAtermWorkerSpillCommand<T extends { type: string }>(
  msg: T
): msg is T & AtermWorkerSpillCommand {
  return (
    msg.type === 'spillCanvasInit' ||
    msg.type === 'spillOverlayBox' ||
    msg.type === 'spillPaneRects' ||
    msg.type === 'spillUnregister' ||
    msg.type === 'spillRelease'
  )
}
