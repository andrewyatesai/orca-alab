// Message protocol (v2, shared worker) for the aterm render worker. ONE worker hosts
// the engines for ALL worker-path panes, keyed by paneId: each engine owns its pane's
// transferred OffscreenCanvas and does parse + render + search + cursor-blink +
// selection + link-detection off the renderer main thread. The main thread keeps NO
// engine: per pane it reads a synchronous STATE snapshot the worker pushes each frame,
// posts mutations as commands, and uses an id-correlated query channel for the few
// cold reads that need off-screen history or post-mutation freshness.
//
// Wire shape: pane-scoped commands/events are the v1 message types intersected with
// `{ paneId }` (`AtermWorkerPaneCommand & { paneId: number }`), stamped by the shared
// worker manager's per-pane `post` — so per-pane senders (the worker-backed term, the
// query channel) stay paneId-free. Worker-SCOPED messages (`fonts`, `booted`, `crash`)
// have no paneId: fonts are sent ONCE per worker generation and kept resident so pane
// inits never re-ship the multi-MB faces, and a crash retires the whole worker.
//
// This file is types-only so the worker and the main-side manager/loader share one
// contract without importing each other's runtime.

import type { AtermThemeColors } from './aterm-theme-colors'

// ── Worker-scoped requests (main → worker, no paneId) ─────────────────────────────

/** The immutable font faces every engine in this worker seeds from, sent ONCE per
 *  worker generation BEFORE the first pane init. The worker keeps them resident so
 *  per-pane inits carry no font bytes at all; the engine-side content-keyed intern
 *  registry then dedupes the bytes across engines within each wasm module. */
export type AtermWorkerFonts = {
  type: 'fonts'
  /** JetBrains-Mono bytes — the engines' built-in primary face. */
  primary: Uint8Array
  /** Optional CJK + non-Latin fallback faces (same bytes the main path injects via
   *  set_fallback_font/add_fallback_font — the MONOCHROME glyph path). */
  fallbacks: Uint8Array[]
  /** Optional OS colour-emoji face (set_emoji_font — the sbix/COLR colour path). Kept
   *  separate from `fallbacks` because the fallback chain renders monochrome. */
  emoji?: Uint8Array
  /** Optional monochrome SYMBOL face (set_symbol_font — the media/technical-glyph tier,
   *  ⏸⏹⏺). Consulted after the fallback chain misses; parity with the native engine. */
  symbol?: Uint8Array
}

// ── Pane-scoped commands (main → worker; wire form adds `paneId`) ─────────────────

/** Engine construction params (sent once per pane, with the transferred canvas).
 *  Fonts are NOT here — the worker seeds every engine from its resident `fonts`. */
export type AtermWorkerInit = {
  type: 'init'
  /** Which engine owns the OffscreenCanvas: 'cpu' (aterm-wasm: rasterize → 2d blit)
   *  or 'gpu' (aterm-gpu-web: WebGL2 present, no readback). The worker falls back to
   *  'cpu' (via a 'fallback' message) if it can't acquire WebGL in the worker. */
  engine: 'cpu' | 'gpu'
  /** The pane canvas, transferred via transferControlToOffscreen(). */
  canvas: OffscreenCanvas
  rows: number
  cols: number
  /** Device-pixel cell font size (already dpr-scaled by the caller). */
  fontPx: number
  /** Cell line-height multiplier (the user's terminalLineHeight; 1 = engine default). */
  lineHeight: number
  /** Full theme: constructor colours + 16-ANSI palette + reply defaults. */
  themeColors: AtermThemeColors
}

/** Feed PTY/replay output (string; the worker encodes into wasm memory). */
export type AtermWorkerProcess = { type: 'process'; data: string }
/** Re-render the current grid into the OffscreenCanvas (coalesced to one rAF frame). */
export type AtermWorkerDraw = { type: 'draw' }
export type AtermWorkerResize = { type: 'resize'; rows: number; cols: number }
/** Re-derive cell metrics at a new device-pixel font size (dpr / font change). */
export type AtermWorkerSetPx = { type: 'setPx'; px: number }
/** Re-derive the cell box height at a new line-height multiplier. */
export type AtermWorkerSetLineHeight = { type: 'setLineHeight'; lineHeight: number }
/** Enable/disable ligature shaping (terminalLigatures); forces a full repaint. */
export type AtermWorkerSetLigatures = { type: 'setLigatures'; on: boolean }
/** Set the scrollback history line limit (terminalScrollbackBytes); 0 = unlimited. */
export type AtermWorkerSetScrollbackLimit = { type: 'setScrollbackLimit'; lines: number }
/** Set the DEFAULT cursor style as a DECSCUSR param 1–6 (terminalCursorStyle); does not
 *  clobber an app's live DECSCUSR. */
export type AtermWorkerSetDefaultCursorStyle = { type: 'setDefaultCursorStyle'; param: number }
/** Per-cell WCAG minimum-contrast fg floor (minimumContrastRatio); <= 1 turns it off. */
export type AtermWorkerSetMinimumContrast = { type: 'setMinimumContrast'; ratio: number }
/** Double-click word separators (terminalWordSeparator); null restores the engine's
 *  default word logic. */
export type AtermWorkerSetWordSeparators = {
  type: 'setWordSeparators'
  separators: string | null
}
/** DEFAULT-background alpha (terminalBackgroundOpacity); 1 = opaque (engine default). */
export type AtermWorkerSetBackgroundOpacity = { type: 'setBackgroundOpacity'; opacity: number }
/** Cursor-fill alpha (terminalCursorOpacity); 1 = opaque (engine default). */
export type AtermWorkerSetCursorOpacity = { type: 'setCursorOpacity'; opacity: number }
/** Enable/disable the Kitty keyboard capability (per-pane static ConPTY policy;
 *  disabled → CSI ? u unanswered, pushes ignored, keyboard_mode carries no kitty bits). */
export type AtermWorkerSetKittyKeyboardEnabled = {
  type: 'setKittyKeyboardEnabled'
  enabled: boolean
}
/** Push the OS color scheme (light/dark). The engine queues a CSI ?997 update when the
 *  scheme changes and the app enabled DEC 2031; the worker drains it to the reply channel. */
export type AtermWorkerSetColorScheme = { type: 'setColorScheme'; dark: boolean }
/** MASTER sparkle-words switch (terminalEffectsSparkleWords); off restores
 *  byte-identical output on the next render. */
export type AtermWorkerSetSparkleWordsEnabled = { type: 'setSparkleWordsEnabled'; on: boolean }
/** Per-class sparkle gates (profanity nova / feline cat / orca splash / emphasis ink). */
export type AtermWorkerSetSparkleClasses = {
  type: 'setSparkleClasses'
  profanity: boolean
  feline: boolean
  orca: boolean
  emphasis: boolean
}
/** OS prefers-reduced-motion → the engine's static (non-animating) sparkle path. */
export type AtermWorkerSetSparkleReducedMotion = { type: 'setSparkleReducedMotion'; on: boolean }
/** Cursor aurora config (terminalEffectsCursorGlow/-Style); null color/accent derive
 *  from the theme cursor in the engine, like the native app. */
export type AtermWorkerSetCursorGlow = {
  type: 'setCursorGlow'
  enabled: boolean
  style: string
  color: number | null
  accent: number | null
  durationMs: number
  length: number
  intensity: number
  radius: number
  ring: boolean
}
/** Pane focus for the effects idle one-shots (an unfocused pane fires no blinks). */
export type AtermWorkerSetEffectsFocused = { type: 'setEffectsFocused'; focused: boolean }
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
/** Authorize/revoke OSC 9/99/777 desktop notifications on the engine (fail-closed
 *  until authorized; synced from the user's notification settings). */
export type AtermWorkerSetNotificationsAuthorized = {
  type: 'setNotificationsAuthorized'
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
/** Swap this pane's primary font face (terminalFontFamily) + reflow once its bytes
 *  load. Carries bytes per pane (a custom family is per-pane user state); the engine
 *  intern registry dedupes identical bytes across panes, so the transfer is transient. */
export type AtermWorkerSetPrimaryFont = { type: 'setPrimaryFont'; bytes: Uint8Array }
/** Swap the SGR-bold face (the family's real bold style). Optional companion to
 *  setPrimaryFont — never sent when the family ships no bold face, so the engine
 *  keeps its synthetic embolden. */
export type AtermWorkerSetBoldFont = { type: 'setBoldFont'; bytes: Uint8Array }
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
 *  'queryResult' event correlated by id (ids are per pane — the pane envelope keeps
 *  different panes' counters from colliding). */
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
    // Parse fence: answered inline by the worker loop, so a resolved reply proves
    // every message posted before it (process bytes + their side-channel replies)
    // has been handled. The replay guard keys its drop window on this.
    | 'flush'
  /** kind-specific numeric arg (scrollbackRows / row / etc.). */
  arg?: number
  /** kind-specific second numeric arg (col for cellText/cellWide/linkAt). */
  arg2?: number
}
/** GPU acquire failed for this pane — rebuild it as CPU on the SAME canvas (it can't
 *  be re-transferred) reusing the stored init params, so it still renders off-main. */
export type AtermWorkerFallback = { type: 'fallback' }
/** Free this pane's engine + drop its worker-side state. Other panes are untouched. */
export type AtermWorkerDispose = { type: 'dispose' }

/** Every pane-scoped command, paneId-free — what per-pane senders (the worker-backed
 *  term, the query channel) build; the manager's per-pane post stamps the paneId. */
export type AtermWorkerPaneCommand =
  | AtermWorkerInit
  | AtermWorkerProcess
  | AtermWorkerDraw
  | AtermWorkerResize
  | AtermWorkerSetPx
  | AtermWorkerSetLineHeight
  | AtermWorkerSetLigatures
  | AtermWorkerSetScrollbackLimit
  | AtermWorkerSetDefaultCursorStyle
  | AtermWorkerSetMinimumContrast
  | AtermWorkerSetWordSeparators
  | AtermWorkerSetBackgroundOpacity
  | AtermWorkerSetCursorOpacity
  | AtermWorkerSetKittyKeyboardEnabled
  | AtermWorkerSetColorScheme
  | AtermWorkerSetSparkleWordsEnabled
  | AtermWorkerSetSparkleClasses
  | AtermWorkerSetSparkleReducedMotion
  | AtermWorkerSetCursorGlow
  | AtermWorkerSetEffectsFocused
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
  | AtermWorkerSetNotificationsAuthorized
  | AtermWorkerSetDrawSuspended
  | AtermWorkerSetCursorBlinkPhase
  | AtermWorkerSetCursorHollow
  | AtermWorkerSetHover
  | AtermWorkerSearchFind
  | AtermWorkerSearchNext
  | AtermWorkerSearchPrev
  | AtermWorkerSearchClear
  | AtermWorkerSetPrimaryFont
  | AtermWorkerSetBoldFont
  | AtermWorkerMouseEncode
  | AtermWorkerQuery
  | AtermWorkerFallback
  | AtermWorkerDispose

/** Pane lifecycle (the worker entry owns these: registry create / CPU rebuild /
 *  engine free); every other pane command is dispatched to the pane's runtime. */
export type AtermWorkerPaneLifecycle = AtermWorkerInit | AtermWorkerFallback | AtermWorkerDispose
export type AtermWorkerPaneRuntimeCommand = Exclude<
  AtermWorkerPaneCommand,
  AtermWorkerPaneLifecycle
>

/** Everything the main thread posts to the worker (the wire union). */
export type AtermWorkerRequest = AtermWorkerFonts | (AtermWorkerPaneCommand & { paneId: number })

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
  /** Total wasm linear memory reserved in the worker (module-wide; all engines
   *  share it — fonts intern once per module). The E1 font-dedup gate reads the
   *  marginal growth per pane from here: wasm memory only grows, so the signal
   *  is deterministic where process RSS drowns in GC noise. */
  wasmHeapBytes: number
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
  /** Device-pixel rects of all ON-SCREEN matches for the main-thread overlay (the
   *  worker owns the match set); `active` flags the one painted in the stronger tone. */
  searchMatchRects: { x: number; y: number; width: number; height: number; active: boolean }[]
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
/** Queued OSC 9/99/777 desktop notifications as a JSON string
 *  `[{id,title,body,urgency},...]`; posted per chunk like OSC app-events. */
export type AtermWorkerNotifications = { type: 'notifications'; events: string }
/** A BEL fired this chunk. */
export type AtermWorkerBell = { type: 'bell' }
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
export type AtermWorkerError = {
  type: 'error'
  phase: 'init'
  message: string
}

/** Every pane-scoped event, paneId-free — what the worker's per-pane post builds;
 *  the entry stamps the paneId and the manager routes on it. */
export type AtermWorkerPaneEvent =
  | AtermWorkerState
  | AtermWorkerReply
  | AtermWorkerOsc
  | AtermWorkerNotifications
  | AtermWorkerBell
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

/** Everything the worker posts back to the main thread (the wire union). */
export type AtermWorkerMessage =
  | AtermWorkerBooted
  | AtermWorkerCrash
  | (AtermWorkerPaneEvent & { paneId: number })
