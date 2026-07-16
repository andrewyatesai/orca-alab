import {
  createAtermSpillScratchReader,
  hasAtermSpillExports,
  type AtermSpillEngineReads,
  type AtermSpillScratchReader
} from './aterm-spill-engine-read'
import {
  adoptSpillPaneGeometry,
  clearSpillRectsAndReblit,
  createSpillPaneState,
  ensureSpillScratch,
  getSpill2dContext,
  reblitAllSpillPanes,
  runSpillClearUnionPass,
  spillGeometryEquals,
  type CreateSpillScratchCanvas,
  type SpillCanvas2D,
  type SpillPaneState,
  type SpillScratchCanvas
} from './aterm-spill-pane-scratch'
import type {
  AtermWorkerSpillBox,
  AtermWorkerSpillCanvasInit,
  AtermWorkerSpillCommand
} from './aterm-worker-spill-protocol'

// The WORKER-side cross-pane spill compositor (stage 4): one registry of pane
// spill states with RETAINED scratches over ONE transferred overlay canvas, so
// all compositing stays off the renderer main thread and rides the existing
// frame clocks (shared-rAF flush epilogue + the eager presentNow tail) — zero
// new timers. Reuses the stage-3 building blocks: the strip/ImageData/rev read
// (aterm-spill-engine-read) and the clear-union + intersect-expansion pass
// (aterm-spill-pane-scratch), so both compositors erase and recover pixels by
// the identical rules.

/** What the worker entry resolves for a live pane (kept injectable so this
 *  module never imports the pane runtime). */
export type WorkerSpillPaneSource = {
  engine: object
  memory: { readonly buffer: ArrayBufferLike } | null
  chrome: { pad: number; head: number }
}

export type AtermWorkerSpillCompositor = {
  /** Route one wire message (the entry calls this BEFORE per-pane dispatch). */
  dispatch: (msg: AtermWorkerSpillCommand & { paneId: number }) => void
  /** drawNow hook, immediately after term.render(): iff chrome≠0 AND spill_rev
   *  advanced, push the pane onto the swap-cleared dirty array. */
  markPaneDirty: (paneId: number) => void
  /** The per-painted-state pass: swap-clears the dirty array, so the flush
   *  epilogue + the presentNow/postNow tail can BOTH invoke it (idempotent). */
  runSpillPass: () => void
  /** Pane close mid-burn: clear its strips with its registry entry. */
  handlePaneDisposed: (paneId: number) => void
}

type WorkerSpillPane = {
  state: SpillPaneState
  paneId: number | null
  /** The engine the reader was built over — a GPU→CPU fallback swaps engines
   *  under the same paneId, so the reader is rebuilt on identity change. */
  engine: object | null
  reader: AtermSpillScratchReader | null
  retryRev: number | null
  retriesLeft: number
}

// Bounded retry per revision, mirroring the stage-3 blit: a geometry/export
// byte mismatch converges in ~one frame; an engine that can never match its
// measured box must not spin render-only frames forever.
const MAX_SKIPPED_PASS_RETRIES = 3

export function createAtermWorkerSpillCompositor(deps: {
  resolvePane: (paneId: number) => WorkerSpillPaneSource | null
  /** Re-arm a render-only frame so a skipped pass retries (bounded). */
  requestRenderRetry?: (paneId: number) => void
  /** Test seam; defaults to OffscreenCanvas (this module runs in the worker). */
  createScratchCanvas?: CreateSpillScratchCanvas
}): AtermWorkerSpillCompositor {
  const createScratch: CreateSpillScratchCanvas =
    deps.createScratchCanvas ?? ((width, height) => new OffscreenCanvas(width, height))
  const panes = new Map<string, WorkerSpillPane>()
  const keyByPaneId = new Map<number, string>()
  // The swap-cleared dirty array (+ a Set so a pane marks once per frame).
  const dirty: number[] = []
  const dirtySet = new Set<number>()
  let canvas: SpillScratchCanvas | null = null
  let ctx: SpillCanvas2D | null = null
  // Monotone across canvas generations; inits at or below it are DEAD-EPOCH
  // frames from a retired canvas and are dropped.
  let epoch = -1
  let box: AtermWorkerSpillBox = { widthPx: 0, heightPx: 0 }

  const paneStates = (): Iterable<SpillPaneState> => [...panes.values()].map((pane) => pane.state)

  const spillReadsOf = (
    pane: WorkerSpillPane,
    src: WorkerSpillPaneSource
  ): AtermSpillEngineReads | null => {
    if (pane.engine !== src.engine || !pane.reader) {
      if (!src.memory || !hasAtermSpillExports(src.engine)) {
        return null
      }
      pane.engine = src.engine
      pane.reader = createAtermSpillScratchReader(
        src.engine as unknown as AtermSpillEngineReads,
        src.memory
      )
    }
    return pane.engine as unknown as AtermSpillEngineReads
  }

  const recompositeAll = (): void => {
    if (!canvas || !ctx) {
      return
    }
    ctx.clearRect(0, 0, canvas.width, canvas.height)
    reblitAllSpillPanes(ctx, paneStates())
  }

  const applyBox = (next: AtermWorkerSpillBox): void => {
    box = { widthPx: next.widthPx, heightPx: next.heightPx }
    if (canvas && (canvas.width !== box.widthPx || canvas.height !== box.heightPx)) {
      // Resizing the backing store implicitly clears it.
      canvas.width = box.widthPx
      canvas.height = box.heightPx
    }
  }

  const markPaneDirty = (paneId: number): void => {
    const paneKey = keyByPaneId.get(paneId)
    const pane = paneKey === undefined ? undefined : panes.get(paneKey)
    if (!pane) {
      return
    }
    const src = deps.resolvePane(paneId)
    if (!src || (src.chrome.pad <= 0 && src.chrome.head <= 0)) {
      return
    }
    const reads = spillReadsOf(pane, src)
    if (!reads || pane.reader?.lastBlittedRev() === reads.spill_rev()) {
      return
    }
    if (!dirtySet.has(paneId)) {
      dirtySet.add(paneId)
      dirty.push(paneId)
    }
  }

  const runPaneSpill = (paneId: number): void => {
    const paneKey = keyByPaneId.get(paneId)
    const pane = paneKey === undefined ? undefined : panes.get(paneKey)
    const src = pane ? deps.resolvePane(paneId) : null
    if (!pane || !src) {
      return
    }
    const reads = spillReadsOf(pane, src)
    const geometry = pane.state.geometry
    if (!reads || !pane.reader) {
      return
    }
    const retry = (rev: number): void => {
      if (pane.retryRev !== rev) {
        pane.retryRev = rev
        pane.retriesLeft = MAX_SKIPPED_PASS_RETRIES
      }
      if (pane.retriesLeft > 0) {
        pane.retriesLeft--
        deps.requestRenderRetry?.(paneId)
      }
    }
    const rev = reads.spill_rev()
    if (rev === pane.reader.lastBlittedRev()) {
      return
    }
    // Hidden/unmeasured geometry paints nothing; retry so a settling burn's
    // FINAL band still lands once the measure catches up (geometry pushes also
    // flush directly — this covers the mid-resize byte-mismatch window).
    if (!geometry?.visible || geometry.outsideRects.length === 0) {
      retry(rev)
      return
    }
    if (!ensureSpillScratch(pane.state, createScratch) || !pane.state.scratchCtx) {
      retry(rev)
      return
    }
    pane.reader.beginPass(rev)
    const dirtyRects = pane.reader.read({
      ctx: pane.state.scratchCtx,
      strips: pane.state.stripSlots
    })
    if (!pane.reader.consumedRev()) {
      retry(rev)
      return
    }
    // No overlay canvas yet: the scratch stays refreshed and the adopt-time
    // recomposite blits the latest state (same rule as the in-process pass).
    if (!ctx || !dirtyRects || dirtyRects.length === 0) {
      return
    }
    runSpillClearUnionPass(ctx, pane.state, geometry, dirtyRects, paneStates())
  }

  const runSpillPass = (): void => {
    if (dirty.length === 0) {
      return
    }
    // Swap-clear FIRST: the epilogue and the eager presentNow tail can both
    // land in one task; the second invocation sees an empty array and no-ops.
    const run = dirty.splice(0, dirty.length)
    dirtySet.clear()
    for (const paneId of run) {
      runPaneSpill(paneId)
    }
  }

  const removePane = (paneKey: string): void => {
    const pane = panes.get(paneKey)
    if (!pane) {
      return
    }
    panes.delete(paneKey)
    if (pane.paneId !== null && keyByPaneId.get(pane.paneId) === paneKey) {
      keyByPaneId.delete(pane.paneId)
      dirtySet.delete(pane.paneId)
    }
    // Clear the departing pane's strips and restore any neighbor pixels the
    // clear touched (the pane is already out of the registry, so its own
    // scratch never re-blits).
    if (ctx && pane.state.prevDrawnRects.length > 0) {
      clearSpillRectsAndReblit(ctx, pane.state.prevDrawnRects, paneStates())
    }
  }

  const handleCanvasInit = (msg: AtermWorkerSpillCanvasInit): void => {
    if (msg.epoch <= epoch) {
      // Dead epoch: this canvas generation was already superseded/released.
      return
    }
    epoch = msg.epoch
    canvas = msg.canvas
    ctx = getSpill2dContext(msg.canvas)
    applyBox(msg.box)
    recompositeAll()
  }

  const dispatch = (msg: AtermWorkerSpillCommand & { paneId: number }): void => {
    switch (msg.type) {
      case 'spillCanvasInit':
        handleCanvasInit(msg)
        return
      case 'spillOverlayBox':
        if (msg.epoch === epoch && canvas) {
          applyBox(msg.box)
          recompositeAll()
        }
        return
      case 'spillPaneRects': {
        let pane = panes.get(msg.paneKey)
        if (!pane) {
          pane = {
            state: createSpillPaneState({ chromePadPx: 0, chromeHeadPx: 0 }),
            paneId: null,
            engine: null,
            reader: null,
            retryRev: null,
            retriesLeft: 0
          }
          panes.set(msg.paneKey, pane)
        }
        // (Re)bind the sending engine — a pane rebuild can move a paneKey to a
        // new paneId; the old reverse mapping must not shadow it.
        if (pane.paneId !== msg.paneId) {
          if (pane.paneId !== null && keyByPaneId.get(pane.paneId) === msg.paneKey) {
            keyByPaneId.delete(pane.paneId)
          }
          pane.paneId = msg.paneId
        }
        keyByPaneId.set(msg.paneId, msg.paneKey)
        if (!spillGeometryEquals(pane.state.geometry, msg.geometry)) {
          adoptSpillPaneGeometry(pane.state, msg.geometry)
          // Geometry push rule: full clear + re-blit ALL panes from scratches.
          recompositeAll()
        }
        // A burn that settled before this bind/measure landed still owes its
        // final band: mark + flush now (message task — still off-main).
        markPaneDirty(msg.paneId)
        runSpillPass()
        return
      }
      case 'spillUnregister':
        removePane(msg.paneKey)
        return
      case 'spillRelease':
        if (msg.epoch >= epoch && canvas) {
          ctx?.clearRect(0, 0, canvas.width, canvas.height)
          canvas = null
          ctx = null
        }
    }
  }

  return {
    dispatch,
    markPaneDirty,
    runSpillPass,
    handlePaneDisposed: (paneId) => {
      const paneKey = keyByPaneId.get(paneId)
      if (paneKey !== undefined) {
        removePane(paneKey)
      }
    }
  }
}
