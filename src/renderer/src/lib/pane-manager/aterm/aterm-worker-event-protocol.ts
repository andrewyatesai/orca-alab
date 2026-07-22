// Worker → main half of the aterm render-worker wire contract (v2, shared worker):
// pane-scoped events (frame STATE snapshots, PTY replies, OSC/notification drains,
// query results) plus the worker-scoped lifecycle events (booted/crash/font misses).
// Split from aterm-render-worker-protocol so neither half crowds the max-lines cap;
// that file re-exports everything here and stays the contract's single entry point.
//
// This file is types-only so the worker and the main-side manager/loader share one
// contract without importing each other's runtime.

import type { AtermSearchMarkerModel } from './aterm-search-marker-model'
import type { AtermFontClass } from './aterm-worker-font-protocol'

// ── Pane-scoped events (worker → main; wire form adds `paneId`) ───────────────────

/** A changed visible display row, pushed when its content changes so the main side's
 *  rolling grid mirror can answer stuck-sync content reads (row/cell text). Omitted
 *  for panes whose content reads can all await (then the query channel serves them). */
export type AtermWorkerGridRow = {
  /** Visible display row index (0 = top of viewport). */
  y: number
  text: string
  wrapped: boolean
  /** Logical length (last non-empty cell + 1). */
  len: number
  /** Per-column cell width digit string ('0' spacer / '1' normal / '2' wide lead). */
  widths: string
}

/** A detected link span on a visible row (for synchronous link_at hit-testing). */
export type AtermWorkerLinkSpan = {
  row: number
  startCol: number
  endCol: number
  url: string
  kind: number
}

/** Cacheable engine state pushed after each frame so the main thread's per-frame
 *  reads (draw/follow-bottom/input gating) and snapshot-backed controller reads stay
 *  synchronous without a round-trip. Mirrors the AtermPaneController read surface. */
export type AtermWorkerState = {
  type: 'state'
  /** Which engine produced this frame (after a possible GPU→CPU fallback). */
  engine: 'cpu' | 'gpu'
  /** Total wasm linear memory reserved in the worker (module-wide; all engines
   *  share it — fonts intern once per module). The E1 font-dedup gate reads the
   *  marginal growth per pane from here: wasm memory only grows, so the signal
   *  is deterministic where process RSS drowns in GC noise. */
  wasmHeapBytes: number
  /** Framebuffer device-pixel size after the last render. INCLUDES the chrome
   *  below when it's non-zero (the frame is [head][pad][grid][pad]). */
  width: number
  height: number
  /** Window-space effects chrome (device px; 0 when none): the grid renders at
   *  offset (pad, pad+head) inside the frame — grid-relative consumers must
   *  subtract these from the frame dims / pointer coords. */
  chromePadPx: number
  chromeHeadPx: number
  cols: number
  rows: number
  cellWidth: number
  cellHeight: number
  /** Lines scrolled up from the live bottom (0 = at bottom). */
  displayOffset: number
  displayOriginAbsolute: number
  cursorX: number
  cursorY: number
  cursorStyle: number
  baseY: number
  isAltScreen: boolean
  bracketedPasteMode: boolean
  isMouseTracking: boolean
  mouseWantsMotion: boolean
  mouseWantsAnyMotion: boolean
  isFocusEventMode: boolean
  isColorSchemeUpdatesMode: boolean
  /** DECCKM (application cursor keys) — drives arrow/Home/End encoding per keystroke. */
  isAppCursorMode: boolean
  /** DEC 1007 (alternate scroll) — wheel on the alt screen sends arrow keys. */
  isAlternateScroll: boolean
  /** Engine KeyboardMode bitflags (kitty/modifyOtherKeys/DECCKM) for the main thread's
   *  synchronous encode_key_with_mode; one-frame staleness accepted like the flags above. */
  keyboardModeBits: number
  isReady: boolean
  /** OSC 0/2 title, or null. */
  title: string | null
  /** Live OSC 12 cursor colour (packed 0x00RRGGBB), or null while unset / after
   *  OSC 112 — the glow/trail colour follow reads it like the mode flags above. */
  cursorColor: number | null
  /** Current selection range in display cells, or null. */
  selectionRange: { startX: number; startY: number; endX: number; endY: number } | null
  /** Selection text. Pushed ONLY when the selection range changes (a large selection is
   *  expensive to re-materialize + clone every frame); `undefined` on an unchanged frame,
   *  where the main side keeps the prior value. */
  selectionText?: string
  /** Link span under the last setHover position, or null — the worker paints its
   *  underline; main reads it only for the (rare) sync controller.linkAt consumer. */
  hoverLink: AtermWorkerLinkSpan | null
  /** Hover affordance the worker derived from the last setHover ('pointer'/''). */
  hoverCursor: string
  /** Search: total matches, 1-based active index (0 = none), active-match device rect. */
  searchCount: number
  searchActiveIndex: number
  searchActiveRect: { x: number; y: number; width: number; height: number } | null
  /** Result versioning: bumped on every re-index, so a result-set change is
   *  detectable even when count/active happen to be identical. */
  searchResultsVersion: number
  /** True while the worker's cost gate serves results older than the buffer content
   *  (an expensive index is not rebuilt per streaming frame; a trailing re-index
   *  always lands the final refresh). The UI surfaces this as the stale indicator. */
  searchResultsStale: boolean
  /** Engine truncated the index (eviction / match cap, E9a): searchCount is a floor — "N+". */
  searchResultsIncomplete: boolean
  /** Echo of the last APPLIED find's request generation — its searchFind query id
   *  (0 before any find). The main side treats results as pending while its newest
   *  issued find id is ahead. */
  searchGeneration: number
  /** Scrollbar match markers: bounded track fractions derived in the worker from the
   *  FULL sorted match list (the on-screen searchMatchRects can't place off-screen ticks). */
  searchMarkers: AtermSearchMarkerModel
  /** The engine exports the spill surface (spill_rev/spill_ptr/...): the loader
   *  reads the FIRST snapshot's value to flip the cross-pane spill seam live. */
  spillExportCapable: boolean
  /** Device-pixel rects of all ON-SCREEN matches for the main-thread overlay (the
   *  worker owns the match set); `active` flags the one painted in the stronger tone. */
  searchMatchRects: { x: number; y: number; width: number; height: number; active: boolean }[]
  /** Changed visible rows since the last state (empty when unchanged or query-channel-served;
   *  the P7 churn rate-limit may withhold rows mid-fling — a settle STATE re-syncs). */
  dirtyRows: AtermWorkerGridRow[]
  /** Predictive-echo ghost cells for the main-thread overlay to paint dim+underlined
   *  (`[row, col, codepoint]` triples in active-grid display coords), empty when
   *  prediction is off / nothing pends / scrolled into history. Display-only: the
   *  ghosts never touch the real grid (dirtyRows), so they stay password-safe. */
  predictOverlay: Uint32Array
  /** Ms until the oldest pending guess self-expires (the glitch flush), or null when
   *  none pends. The controller arms ONE main-thread timer from this + repaints, so a
   *  stale ghost is erased even with no further input/output. Read AFTER the worker's
   *  expiry self-heal (predict_overlay), so it's never a permanently-past instant that
   *  would pin the timer (the native stranded-deadline 100%-CPU invariant). */
  predictDeadlineMs: number | null
}

/** Engine query replies (DA/DSR/CPR/colour/CSI 14t-16t) to forward to the PTY.
 *  Posted immediately per processed chunk (NOT coalesced) so none are dropped and
 *  ordering is preserved. */
export type AtermWorkerReply = { type: 'reply'; data: string }
/** Queued OSC app-events as a JSON string `[[code,payload],...]`; posted per chunk. */
export type AtermWorkerOsc = { type: 'osc'; events: string }
/** Queued OSC 9/99/777 desktop notifications as a JSON string
 *  `[{id,title,body,urgency},...]`; posted per chunk like OSC app-events. */
export type AtermWorkerNotifications = { type: 'notifications'; events: string }
/** A BEL fired this chunk. */
export type AtermWorkerBell = { type: 'bell' }
/** The engine's KeyboardMode bits changed while processing a chunk. Posted
 *  immediately (per chunk, like replies) — NOT with the coalesced frame STATE —
 *  so the main thread's synchronous keyboard-mode mirror is fresh before the
 *  next keystroke even when the app flips modes and then goes idle (no frame
 *  would ever refresh the snapshot). */
export type AtermWorkerKeyboardModeBits = { type: 'keyboardModeBits'; bits: number }
/** Debounced serialized-buffer snapshot the worker pushes while idle, so the main
 *  thread can read a recent buffer SYNCHRONOUSLY at shutdown layout-capture (which
 *  can't await). Slightly stale (the debounce window); the awaitable save paths use
 *  the fresh query round-trip instead. */
export type AtermWorkerSerializedCache = {
  type: 'serializedCache'
  /** Full-buffer serialize (capped scrollback) — replayable ANSI. */
  full: string
  /** Scrollback-history-only serialize. */
  scrollback: string
}
/** Result of a 'query' (serialize / content / selectionText), correlated by id. */
export type AtermWorkerQueryResult = {
  type: 'queryResult'
  id: number
  /** Stringified result; numeric kinds (rowLen) are JSON numbers, null when absent. */
  value: string | number | boolean | null
}
/** A PANE-scoped failure: its engine build (GPU acquire / CPU init) failed. The
 *  loader answers with a 'fallback' so the pane rebuilds as CPU on the same canvas.
 *  Worker-fatal failures are NOT here — they post the worker-scoped 'crash'. */
export type AtermWorkerError = { type: 'error'; phase: 'init'; message: string }

/** Every pane-scoped event, paneId-free — what the worker's per-pane post builds;
 *  the entry stamps the paneId and the manager routes on it. */
export type AtermWorkerPaneEvent =
  | AtermWorkerState
  | AtermWorkerReply
  | AtermWorkerOsc
  | AtermWorkerNotifications
  | AtermWorkerBell
  | AtermWorkerKeyboardModeBits
  | AtermWorkerSerializedCache
  | AtermWorkerQueryResult
  | AtermWorkerError

// ── Worker-scoped events (worker → main, no paneId) ───────────────────────────────

/** Posted once when the worker receives its 'fonts' message (always the first message
 *  a fresh worker gets), BEFORE any engine build. Lets the manager/loader tell a
 *  live-but-building worker from a wedged one: the short boot deadline applies only
 *  until this ack; after it each pane waits out its own build under a longer (still
 *  bounded) cap instead of killing a healthy worker under concurrent-open contention. */
export type AtermWorkerBooted = { type: 'booted' }
/** WORKER-fatal failure (an exception escaped the message dispatch — e.g. a wasm
 *  RuntimeError, whose module state is now suspect for EVERY engine in it). The
 *  manager retires the worker and every live pane rebuilds through its context-loss
 *  seam — without this each pane silently freezes at its last frame. */
export type AtermWorkerCrash = { type: 'crash'; message: string }
/** An engine rendered `.notdef` for a char an absent font CLASS would have served
 *  (E1 lazy fonts, drained from the engine's take_missing_font_classes after a
 *  frame). Worker-scoped — fonts are worker-resident, one delivery serves every
 *  pane. The worker posts each class at most once per generation (latched); the
 *  manager answers with a 'fontClass' delivery, self-healing across crashes
 *  because a rebuilt generation simply re-fires on the next miss. */
export type AtermWorkerMissingFontClasses = {
  type: 'missingFontClasses'
  classes: AtermFontClass[]
}

/** Everything the worker posts back to the main thread (the wire union). */
export type AtermWorkerMessage =
  | AtermWorkerBooted
  | AtermWorkerCrash
  | AtermWorkerMissingFontClasses
  | (AtermWorkerPaneEvent & { paneId: number })
