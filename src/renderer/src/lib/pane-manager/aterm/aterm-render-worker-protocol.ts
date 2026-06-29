// Message protocol for the single-engine aterm render worker (plan: aterm-single-
// engine-worker.md). The worker owns the ONLY engine for a pane + its transferred
// OffscreenCanvas, and does parse + render + search + cursor-blink + selection +
// link-detection off the renderer main thread. The main thread keeps NO engine: it
// reads a synchronous STATE snapshot the worker pushes each frame, posts mutations
// as commands, and uses an id-correlated query channel for the few cold reads that
// need off-screen history or post-mutation freshness (serialize / content / copy).
//
// This file is types-only so the worker and the main-side loader share one contract
// without importing each other's runtime.

import type { AtermThemeColors } from './aterm-theme-colors'

// ── Construction ────────────────────────────────────────────────────────────────

/** Engine construction params (sent once on init, with the transferred canvas). */
export type AtermWorkerInit = {
  type: 'init'
  /** Which engine owns the OffscreenCanvas: 'cpu' (aterm-wasm: rasterize → 2d blit)
   *  or 'gpu' (aterm-gpu-web: WebGL2 present, no readback). The worker falls back to
   *  'cpu' (via a 'fallback' message) if it can't acquire WebGL in the worker. */
  engine: 'cpu' | 'gpu'
  /** The pane canvas, transferred via transferControlToOffscreen(). */
  canvas: OffscreenCanvas
  /** JetBrains-Mono bytes (the main thread already fetched them; transferable). */
  fontBytes: Uint8Array
  /** Optional CJK/emoji fallback faces (same bytes the main path injects). */
  fallbackFonts: Uint8Array[]
  rows: number
  cols: number
  /** Device-pixel cell font size (already dpr-scaled by the caller). */
  fontPx: number
  /** Cell line-height multiplier (the user's terminalLineHeight; 1 = engine default). */
  lineHeight: number
  /** Full theme: constructor colours + 16-ANSI palette + reply defaults. */
  themeColors: AtermThemeColors
}

// ── Commands (main → worker) ──────────────────────────────────────────────────────

/** Feed PTY/replay output (string; the worker encodes into wasm memory). */
export type AtermWorkerProcess = { type: 'process'; data: string }
/** Re-render the current grid into the OffscreenCanvas (coalesced to one rAF frame). */
export type AtermWorkerDraw = { type: 'draw' }
export type AtermWorkerResize = { type: 'resize'; rows: number; cols: number }
/** Re-derive cell metrics at a new device-pixel font size (dpr / font change). */
export type AtermWorkerSetPx = { type: 'setPx'; px: number }
/** Re-derive the cell box height at a new line-height multiplier. */
export type AtermWorkerSetLineHeight = { type: 'setLineHeight'; lineHeight: number }
export type AtermWorkerScrollLines = { type: 'scrollLines'; delta: number }
export type AtermWorkerScrollToBottom = { type: 'scrollToBottom' }
export type AtermWorkerScrollToTop = { type: 'scrollToTop' }
export type AtermWorkerScrollToLine = { type: 'scrollToLine'; line: number }
/** Mouse-driven selection: the worker owns the grid selection + paints the
 *  highlight; main posts cell coords from pointer events. word/line return their
 *  text via the next snapshot (and the query channel for copy-on-select). */
export type AtermWorkerSelectionStart = { type: 'selectionStart'; row: number; col: number }
export type AtermWorkerSelectionExtend = { type: 'selectionExtend'; row: number; col: number }
export type AtermWorkerSelectionFinish = { type: 'selectionFinish' }
export type AtermWorkerSelectionWord = { type: 'selectionWord'; row: number; col: number }
export type AtermWorkerSelectionLine = { type: 'selectionLine'; row: number; col: number }
export type AtermWorkerSelectionClear = { type: 'selectionClear' }
/** Granular engine theme/reply-default setters — `applyAtermLiveTheme` fans out to
 *  these on a live theme change; the worker-backed term forwards each so the engine's
 *  palette + OSC 10/11 + CSI 14t/16t reply state stays correct without a pane rebuild. */
export type AtermWorkerThemeSet =
  | { type: 'themeSet'; op: 'theme'; fg: number; bg: number; cursor: number; selection: number }
  | { type: 'themeSet'; op: 'paletteColor'; index: number; r: number; g: number; b: number }
  | { type: 'themeSet'; op: 'defaultForeground'; r: number; g: number; b: number }
  | { type: 'themeSet'; op: 'defaultBackground'; r: number; g: number; b: number }
  | { type: 'themeSet'; op: 'selectionFg'; fg: number | null }
  | { type: 'themeSet'; op: 'cellPixelSize'; width: number; height: number }
export type AtermWorkerSetSelectionInactive = { type: 'setSelectionInactive'; inactive: boolean }
export type AtermWorkerSetSelectionInactiveBg = {
  type: 'setSelectionInactiveBg'
  bg: number | null
}
/** Authorize/revoke OSC 52 clipboard write on the engine (host enforces the setting). */
export type AtermWorkerSetClipboardWriteAuthorized = {
  type: 'setClipboardWriteAuthorized'
  allowed: boolean
}
/** Pause/resume frame painting (hidden-pane gating); the engine still ingests bytes. */
export type AtermWorkerSetDrawSuspended = { type: 'setDrawSuspended'; suspended: boolean }
/** The main-thread cursor-blink timer drives these: toggle the blink phase and the
 *  hollow (unfocused) cursor box; the engine paints them on the next render. */
export type AtermWorkerSetCursorBlinkPhase = { type: 'setCursorBlinkPhase'; on: boolean }
export type AtermWorkerSetCursorHollow = { type: 'setCursorHollow'; hollow: boolean }
/** Hover position for link underline + cursor affordance, or clear when off a link. */
export type AtermWorkerSetHover =
  | { type: 'setHover'; row: number; col: number }
  | { type: 'setHover'; clear: true }
/** Search: the worker runs find/next/prev/clear, paints highlights, and reports
 *  count/activeIndex/rect in the snapshot. */
export type AtermWorkerSearchFind = {
  type: 'searchFind'
  query: string
  caseSensitive: boolean
  isRegex: boolean
}
export type AtermWorkerSearchNext = { type: 'searchNext' }
export type AtermWorkerSearchPrev = { type: 'searchPrev' }
export type AtermWorkerSearchClear = { type: 'searchClear' }
/** Swap the primary font face (terminalFontFamily) + reflow once its bytes load. */
export type AtermWorkerSetPrimaryFont = { type: 'setPrimaryFont'; bytes: Uint8Array }
export type AtermWorkerForceReflow = { type: 'forceReflow' }
/** Encode a mouse report in the worker (the engine owns the protocol). The encoded
 *  bytes are PTY input, so the worker forwards them through the SAME 'reply' channel as
 *  engine query replies → main writes them to the PTY (onReply → inputSink). The
 *  synchronous preventDefault/gate decision uses the snapshot flags, so no response
 *  correlation is needed. */
export type AtermWorkerMouseEncode = {
  type: 'mouseEncode'
  kind: 'press' | 'release' | 'motion' | 'wheel'
  col: number
  row: number
  /** X10 button code (press/release/motion); ignored for wheel. */
  button: number
  /** Modifier byte (Shift=4, Alt=8, Ctrl=16). */
  mods: number
  /** Wheel only: true = wheel-up. */
  up?: boolean
}
/** Cold read needing off-screen history or post-mutation freshness; answered as a
 *  'queryResult' event correlated by id. */
export type AtermWorkerQuery = {
  type: 'query'
  id: number
  kind:
    | 'serialize'
    | 'serializeScrollback'
    | 'selectionText'
    | 'rowText'
    | 'rowLen'
    | 'rowWrapped'
    | 'cellText'
    | 'cellWide'
    | 'linkAt'
  /** kind-specific numeric arg (scrollbackRows / row / etc.). */
  arg?: number
  /** kind-specific second numeric arg (col for cellText/cellWide/linkAt). */
  arg2?: number
}
/** GPU acquire failed in the worker — rebuild as CPU on the SAME canvas (it can't be
 *  re-transferred) reusing the stored init params, so the pane still renders off-main. */
export type AtermWorkerFallback = { type: 'fallback' }
export type AtermWorkerDispose = { type: 'dispose' }

export type AtermWorkerRequest =
  | AtermWorkerInit
  | AtermWorkerProcess
  | AtermWorkerDraw
  | AtermWorkerResize
  | AtermWorkerSetPx
  | AtermWorkerSetLineHeight
  | AtermWorkerScrollLines
  | AtermWorkerScrollToBottom
  | AtermWorkerScrollToTop
  | AtermWorkerScrollToLine
  | AtermWorkerSelectionStart
  | AtermWorkerSelectionExtend
  | AtermWorkerSelectionFinish
  | AtermWorkerSelectionWord
  | AtermWorkerSelectionLine
  | AtermWorkerSelectionClear
  | AtermWorkerThemeSet
  | AtermWorkerSetSelectionInactive
  | AtermWorkerSetSelectionInactiveBg
  | AtermWorkerSetClipboardWriteAuthorized
  | AtermWorkerSetDrawSuspended
  | AtermWorkerSetCursorBlinkPhase
  | AtermWorkerSetCursorHollow
  | AtermWorkerSetHover
  | AtermWorkerSearchFind
  | AtermWorkerSearchNext
  | AtermWorkerSearchPrev
  | AtermWorkerSearchClear
  | AtermWorkerSetPrimaryFont
  | AtermWorkerForceReflow
  | AtermWorkerMouseEncode
  | AtermWorkerQuery
  | AtermWorkerFallback
  | AtermWorkerDispose

// ── Events (worker → main) ────────────────────────────────────────────────────────

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
  /** Framebuffer device-pixel size after the last render. */
  width: number
  height: number
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
  isReady: boolean
  /** OSC 0/2 title, or null. */
  title: string | null
  /** Current selection range in display cells, or null. */
  selectionRange: { startX: number; startY: number; endX: number; endY: number } | null
  /** Selection text (pushed when the selection changes; '' when none). */
  selectionText: string
  /** Link span under the last setHover position, or null — the worker paints its
   *  underline; main reads it only for the (rare) sync controller.linkAt consumer. */
  hoverLink: AtermWorkerLinkSpan | null
  /** Hover affordance the worker derived from the last setHover ('pointer'/''). */
  hoverCursor: string
  /** Search: total matches, 1-based active index (0 = none), active-match device rect. */
  searchCount: number
  searchActiveIndex: number
  searchActiveRect: { x: number; y: number; width: number; height: number } | null
  /** Changed visible rows since the last state (empty when content is unchanged or
   *  when this pane serves content reads via the query channel instead). */
  dirtyRows: AtermWorkerGridRow[]
}

/** Engine query replies (DA/DSR/CPR/colour/CSI 14t-16t) to forward to the PTY.
 *  Posted immediately per processed chunk (NOT coalesced) so none are dropped and
 *  ordering is preserved. */
export type AtermWorkerReply = { type: 'reply'; data: string }
/** Queued OSC app-events as a JSON string `[[code,payload],...]`; posted per chunk. */
export type AtermWorkerOsc = { type: 'osc'; events: string }
/** A BEL fired this chunk. */
export type AtermWorkerBell = { type: 'bell' }
/** Result of a 'query' (serialize / content / selectionText), correlated by id. */
export type AtermWorkerQueryResult = {
  type: 'queryResult'
  id: number
  /** Stringified result; numeric kinds (rowLen) are JSON numbers, null when absent. */
  value: string | number | boolean | null
}
/** A worker failure. `phase: 'init'` (GPU acquire failed) triggers the GPU→CPU
 *  fallback; `phase: 'render'` is logged. */
export type AtermWorkerError = { type: 'error'; phase: 'init' | 'render'; message: string }

/** Everything the worker posts back to the main thread. */
export type AtermWorkerMessage =
  | AtermWorkerState
  | AtermWorkerReply
  | AtermWorkerOsc
  | AtermWorkerBell
  | AtermWorkerQueryResult
  | AtermWorkerError

/** First state after init carries the initial cell metrics so the host can build the
 *  grid; reuses AtermWorkerState. */
export type AtermWorkerResponse = AtermWorkerState
