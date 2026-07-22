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
import type { AtermWorkerRainCommand } from './aterm-worker-rain-protocol'
import type { AtermWorkerSpillCommand } from './aterm-worker-spill-protocol'
import type { AtermWorkerPredictCommand } from './aterm-worker-predict-protocol'
import type {
  AtermFontClass,
  AtermWorkerFontClass,
  AtermWorkerFonts
} from './aterm-worker-font-protocol'

// ── Worker-scoped requests (main → worker, no paneId) ─────────────────────────────

// The once-per-generation font delivery + lazy font-class types live in
// aterm-worker-font-protocol; re-exported so this file stays the wire contract's
// single entry point.
export type { AtermFontClass, AtermWorkerFontClass, AtermWorkerFonts }

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
export type AtermWorkerSetWordSeparators = { type: 'setWordSeparators'; separators: string | null }
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
/** Window-space effects chrome (device px): interior pad per edge + a top-only head
 *  band, so cursor effects (fire) can escape the grid into the chrome. 0/0 restores
 *  the byte-identical exact-fit frame. */
export type AtermWorkerSetChrome = { type: 'setChrome'; pad: number; head: number }
/** Pane focus for the effects idle one-shots (an unfocused pane fires no blinks). */
export type AtermWorkerSetEffectsFocused = { type: 'setEffectsFocused'; focused: boolean }
/** Worker QoS focus signal (R4): which pane the user is typing in. The worker's
 *  command scheduler services the focused pane's interactive work AHEAD of a
 *  background pane's bulk `process`, so a flooding sibling can't starve keystroke
 *  echo. Distinct from setEffectsFocused (that carries effects semantics + is
 *  superseded by the rain tri-state on the worker path); this is pure QoS priority. */
export type AtermWorkerSetFocused = { type: 'setFocused'; focused: boolean }
export type AtermWorkerScrollLines = { type: 'scrollLines'; delta: number }
/** Sub-row scrollback input in device PIXELS (DOM_DELTA_PIXEL wheel; positive reveals
 *  older lines, the scrollLines sign convention). Crosses the seam UNROUNDED: the
 *  engine banks the fractional residual and presents it as a pixel band shift. */
export type AtermWorkerScrollPx = { type: 'scrollPx'; deltaPx: number }
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
/** Search nav/clear: the worker owns the match set and reports count/activeIndex/rect
 *  in the snapshot. A find is NOT a command — it rides the id-correlated 'query'
 *  channel ('searchFind') so the main thread can correlate results to a request
 *  generation and cancel superseded finds. */
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
    // Run a find NOW and answer with `{count, activeIndex}` JSON. Rides the query
    // channel (not a command) so the monotonic id doubles as the request GENERATION:
    // the main thread cancels a superseded find's promise instantly, and the worker
    // skips executing a queued find once a newer one has arrived.
    | 'searchFind'
    // Parse fence: answered inline by the worker loop, so a resolved reply proves
    // every message posted before it (process bytes + their side-channel replies)
    // has been handled. The replay guard keys its drop window on this.
    | 'flush'
  /** kind-specific numeric arg (scrollbackRows / row / searchFind flag bits / etc.). */
  arg?: number
  /** kind-specific second numeric arg (col for cellText/cellWide/linkAt). */
  arg2?: number
  /** kind-specific string arg (the searchFind query text). */
  text?: string
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
  | AtermWorkerRainCommand
  | AtermWorkerSetCursorGlow
  | AtermWorkerSetChrome
  | AtermWorkerSetEffectsFocused
  | AtermWorkerSetFocused
  | AtermWorkerScrollLines
  | AtermWorkerScrollPx
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
  | AtermWorkerSearchNext
  | AtermWorkerSearchPrev
  | AtermWorkerSearchClear
  | AtermWorkerSetPrimaryFont
  | AtermWorkerSetBoldFont
  | AtermWorkerMouseEncode
  | AtermWorkerPredictCommand
  | AtermWorkerQuery
  | AtermWorkerFallback
  | AtermWorkerDispose

/** Pane lifecycle (the worker entry owns these: registry create / CPU rebuild /
 *  engine free); every other pane command is dispatched to the pane's runtime. */
export type AtermWorkerPaneLifecycle = AtermWorkerInit | AtermWorkerFallback | AtermWorkerDispose
// setFocused is worker-entry scheduler bookkeeping, not engine work — excluded from
// the per-pane runtime dispatch (dispatchPaneCommand never sees it).
export type AtermWorkerPaneRuntimeCommand = Exclude<
  AtermWorkerPaneCommand,
  AtermWorkerPaneLifecycle | AtermWorkerSetFocused
>

/** Everything the main thread posts to the worker (the wire union). The spill
 *  family (aterm-worker-spill-protocol) rides pane-stamped but is routed to the
 *  worker-global compositor BEFORE the per-pane dispatch. */
export type AtermWorkerRequest =
  | AtermWorkerFonts
  | AtermWorkerFontClass
  | ((AtermWorkerPaneCommand | AtermWorkerSpillCommand) & { paneId: number })

// ── Events (worker → main) ────────────────────────────────────────────────────────

// The worker → main half of the contract (frame STATE, replies, query results,
// booted/crash/font misses) lives in aterm-worker-event-protocol; re-exported so
// this file stays the wire contract's single entry point.
export type {
  AtermWorkerGridRow,
  AtermWorkerLinkSpan,
  AtermWorkerState,
  AtermWorkerReply,
  AtermWorkerOsc,
  AtermWorkerNotifications,
  AtermWorkerBell,
  AtermWorkerKeyboardModeBits,
  AtermWorkerSerializedCache,
  AtermWorkerQueryResult,
  AtermWorkerError,
  AtermWorkerPaneEvent,
  AtermWorkerBooted,
  AtermWorkerCrash,
  AtermWorkerMissingFontClasses,
  AtermWorkerMessage
} from './aterm-worker-event-protocol'
