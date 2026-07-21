import {
  ATERM_KEY_EVENT_PRESS,
  encodeKeyEventToBytes,
  type AtermEngineKeyEncoder
} from './aterm-key-encoding'
import { createAtermCompositionView } from './aterm-composition-view'
import {
  shouldNoteAtermKeystroke,
  shouldNoteAtermMatrixRainActivity
} from './aterm-effects-activity-gate'
import { ATERM_RAIN_SIGNAL_CODES } from '../../../../../shared/aterm-rain-signal'
import {
  markTerminalPinnedViewport,
  syncTerminalScrollIntentFromViewport
} from '../terminal-scroll-intent'
import type { TerminalScrollIntentTarget } from '../terminal-scroll-intent-types'
import { encode_key_with_mode } from './aterm_wasm.js'
import type { AtermPredictionEcho } from './aterm-prediction-echo'
import type { AtermTerminal } from './aterm_wasm.js'

/** The engine encoder for a live/worker-backed term: `term.encode_key` when the
 *  engine is in-process (LIVE keyboard mode — exact), else the wasm free
 *  function + the STATE snapshot's `keyboard_mode_bits` (worker path; ≤1-frame
 *  stale — see the attachAtermTextareaInput JSDoc for why that trade is kept). */
export function selectAtermEngineKeyEncoder(term: AtermTerminal): AtermEngineKeyEncoder {
  return typeof term.encode_key === 'function'
    ? (key, mods, eventType, baseLayoutKey) => term.encode_key(key, mods, eventType, baseLayoutKey)
    : (key, mods, eventType, baseLayoutKey) =>
        encode_key_with_mode(key, mods, eventType, baseLayoutKey, term.keyboard_mode_bits)
}

const HOST_KEY_BYTES_DECODER = new TextDecoder()

/** Encode a host-synthesized key PRESS (window-level shortcut policy 'encodeKey'
 *  actions — e.g. Cmd+Backspace / Option+B routed to a kitty/modifyOtherKeys
 *  pane) through the pane's engine, honoring the live keyboard mode so the
 *  ENGINE picks the negotiated dialect (kitty CSI-u vs xterm modifyOtherKeys).
 *  Returns null when the engine has no encoding (caller falls back to legacy
 *  bytes so the chord never goes dead). */
export function encodeAtermKeyForHost(
  term: AtermTerminal,
  key: string,
  mods: number
): string | null {
  const bytes = selectAtermEngineKeyEncoder(term)(key, mods, ATERM_KEY_EVENT_PRESS, null)
  if (!bytes || bytes.length === 0) {
    return null
  }
  return HOST_KEY_BYTES_DECODER.decode(bytes)
}

/** Inputs for the helper-textarea keyboard/text wiring. The textarea is the
 *  app's focus/paste/IME sink (mirrors xterm's helper textarea); this module owns
 *  the keydown/input/composition handlers that turn it into PTY bytes. */
export type AtermTextareaInputDeps = {
  textarea: HTMLTextAreaElement
  term: AtermTerminal
  /** The grid canvas — the IME composition view anchors/paints over it. */
  canvas: HTMLCanvasElement
  /** Live cell metrics in device px + dpr (mutated in place on DPI change). */
  metrics: { dpr: number; cellWidth: number; cellHeight: number }
  /** Live theme (mutated in place on re-theme) for the IME preedit colors. */
  themeColors: { fg: number; bg: number }
  /** Preferred IME anchor cell when an agent CLI parks the real cursor away
   *  from its visible prompt (upstream #7061); null → the engine cursor. */
  getImeAnchor?: () => { row: number; col: number } | null
  /** Current grid rows — the Shift+PageUp/PageDown scrollback page size. */
  getRows: () => number
  /** Repaint after a keyboard-driven scrollback move. */
  redraw: () => void
  /** The pane's scroll-intent target (facade). Shift+PageUp/Down scrolls the engine
   *  directly, so it must record intent through this seam — mirroring the keyboard
   *  handlers' Cmd+Up/Down path — or a later keyed remount snaps the viewport to the
   *  bottom and loses the reading position. Absent → no intent tracking (tests). */
  getScrollIntentTarget?: () => TerminalScrollIntentTarget | null
  /** Predictive-echo controller — fed on the SAME keystroke seam that writes to
   *  the PTY (printable → char, Backspace → backspace, plain Enter → submit) so the
   *  speculative ghost paints ~1 RTT before the echo. Inert when not predict-capable
   *  (worker path) or off; display-only, so it never changes what's sent. */
  predictionEcho?: AtermPredictionEcho
  /** Send encoded bytes (typing/IME) to the PTY raw. */
  inputSink: (data: string) => void
  /** Send PASTED text to the PTY; wraps with \e[200~..\e[201~ when the app has
   *  enabled bracketed paste (DECSET 2004) so editors don't auto-indent/run it. */
  pasteSink: (data: string) => void
  /** Copy the current canvas selection; returns true when something was copied. */
  copySelection: () => boolean
  /** Latest macOptionIsMeta setting (xterm's option of the same name). Read per
   *  press so a live settings toggle takes effect without recreating the pane;
   *  controls whether macOS Option meta-prefixes or composes a glyph. Defaults
   *  to false (the app default) when omitted. */
  getMacOptionIsMeta?: () => boolean
  /** The consumer hook registered via the facade's attachCustomKeyEventHandler
   *  (IME suppression, Ctrl+C interrupt + kitty reset, JIS-yen, scroll intent).
   *  Consulted per keydown BEFORE any engine encoding — xterm's contract — and a
   *  `false` return means the consumer handled/suppressed the key, so nothing is
   *  encoded or sent for it. Read live so a late registration applies. */
  getCustomKeyEventHandler?: () => ((event: KeyboardEvent) => boolean) | null
}

/** Wire the helper textarea to the PTY following xterm's input model:
 *  - keydown handles ONLY non-text keys (Enter, arrows, Ctrl/Alt chords, …),
 *    encoded by the ENGINE encoder (legacy + modifyOtherKeys + kitty, driven by
 *    the terminal's keyboard mode); plain printable chars are NOT sent here.
 *  - the 'input' event handles printable text, paste (setRangeText+InputEvent),
 *    and the IME commit (compositionend) — one route for all text, never doubled.
 *  - keyup encodes releases (event_type=2) for kitty REPORT_EVENT_TYPES apps;
 *    the engine drops them in legacy mode so nothing leaks to normal shells.
 *  Returns a disposer that removes every listener.
 *
 *  In-process the encoder is `term.encode_key` (LIVE keyboard mode — exact). On
 *  the single-engine worker path the engine lives off-thread, so keydowns encode
 *  through the wasm free function `encode_key_with_mode` with the latest STATE
 *  snapshot's `keyboard_mode_bits`, which can lag the engine by up to one frame:
 *  a key pressed in the ~1-frame window right after a TUI flips a keyboard mode
 *  (DECCKM, kitty push/pop) may encode under the previous mode for that ONE
 *  keystroke. We deliberately keep this synchronous read instead of round-tripping
 *  the key to the worker: input bytes go straight to the PTY today, so a
 *  round-trip would (a) queue the keystroke behind pending output in the worker
 *  (laggy Ctrl-C during heavy output) and (b) reorder it against printable
 *  chars/IME still sent synchronously here. The same snapshot-lag caveat applies
 *  to the mouse click-gate (is_mouse_tracking) and the paste wrap
 *  (bracketed_paste_mode); the snapshot is the safest available default. The
 *  free function is always callable here: every load path — including the worker
 *  loader, for fonts — awaits loadAterm() (which inits the wasm module on the
 *  main thread) before this wiring runs. */
export function attachAtermTextareaInput(deps: AtermTextareaInputDeps): { dispose: () => void } {
  const { textarea, term, canvas, metrics, themeColors, getRows, redraw } = deps
  const { inputSink, pasteSink, copySelection, getMacOptionIsMeta, predictionEcho } = deps
  const { getCustomKeyEventHandler, getScrollIntentTarget } = deps
  // Platform-correct copy modifier: Cmd on macOS, Ctrl elsewhere.
  const isMac = typeof navigator !== 'undefined' && navigator.userAgent.includes('Mac')
  let composing = false
  const effectsTerm = term as AtermTerminal & {
    note_keystroke?: () => void
    note_matrix_rain_alt_scroll?: () => void
    note_matrix_rain_signal?: (code: number, weight: number) => void
  }
  const noteEffectsKeystroke = (turnStart = false): void => {
    if (shouldNoteAtermKeystroke(effectsTerm)) {
      effectsTerm.note_keystroke?.()
    }
    if (turnStart && shouldNoteAtermMatrixRainActivity(effectsTerm)) {
      // Submit is an observable boundary, not content: it lets the engine
      // distinguish a same-present first response from an editor repaint.
      effectsTerm.note_matrix_rain_signal?.(ATERM_RAIN_SIGNAL_CODES.turn_start, 4)
    }
  }

  // Worker-backed terms expose no encode_key (no engine on this thread); they
  // encode through the free function + snapshot mode bits (see the JSDoc above).
  const encodeWithEngine: AtermEngineKeyEncoder = selectAtermEngineKeyEncoder(term)
  // Live/snapshot KeyboardMode bits for the report-all printable-routing gate
  // (works on both paths: in-process getter + worker snapshot mirror).
  const getKeyboardModeBits = (): number => term.keyboard_mode_bits

  // Anchors the textarea (and paints the preedit) at the cursor cell while an
  // IME composition is open, so the candidate window opens at the caret.
  const compositionView = createAtermCompositionView({
    canvas,
    textarea,
    term,
    metrics,
    themeColors,
    getAnchorOverride: deps.getImeAnchor
  })

  // Copy the canvas selection for the platform's copy chord; returns true when
  // the chord was handled (so the caller swallows it). On Linux/Windows
  // Ctrl+Shift+C is the EXPLICIT copy shortcut and must NEVER send ^C — it is
  // swallowed even with no selection so it can't leak an interrupt.
  const tryCopyChord = (event: KeyboardEvent): boolean => {
    if (event.key.toLowerCase() !== 'c') {
      return false
    }
    if (isMac) {
      // Cmd+C copies; with no selection it falls through (Cmd doesn't send ^C).
      return event.metaKey ? copySelection() : false
    }
    // Ctrl+Shift+C = explicit copy: always swallow, copy when there's a selection.
    if (event.ctrlKey && event.shiftKey) {
      copySelection()
      return true
    }
    // Plain Ctrl+C copies only when there's a selection; otherwise it falls
    // through so the encoder sends ^C (interrupt).
    return event.ctrlKey ? copySelection() : false
  }

  // xterm's default Shift+PageUp/PageDown pages the SCROLLBACK on the main
  // screen; on the alternate screen the chord falls through to the engine
  // encoder so full-screen apps receive the modified key instead.
  const tryScrollbackPage = (event: KeyboardEvent): boolean => {
    if (event.key !== 'PageUp' && event.key !== 'PageDown') {
      return false
    }
    if (!event.shiftKey || event.ctrlKey || event.altKey || event.metaKey) {
      return false
    }
    if (term.is_alt_screen) {
      return false
    }
    const page = Math.max(1, getRows() - 1)
    // Positive aterm delta reveals older history (up).
    term.scroll_lines(event.key === 'PageUp' ? page : -page)
    redraw()
    // Record scroll intent on the facade — the same seam keyboard-handlers'
    // Cmd+Up/Down uses. Without it a keyed remount / workspace-switch restores to
    // followOutput and snaps the viewport to the bottom (lost reading position).
    // mark-then-sync: sync reclassifies to followOutput when the page lands at the
    // bottom (PageDown), or keeps the pin when it reveals history (PageUp).
    const intentTarget = getScrollIntentTarget?.()
    if (intentTarget) {
      markTerminalPinnedViewport(intentTarget)
      syncTerminalScrollIntentFromViewport(intentTarget, { userInteraction: true })
    }
    return true
  }

  const onKeyDown = (event: KeyboardEvent): void => {
    // Let the IME own keys while a composition is active (checked here AND in the
    // input handler so a composed string is never also sent char-by-char).
    if (event.isComposing || composing) {
      return
    }
    // The copy chord runs BEFORE the consumer hook: the hook's clipboard-bypass
    // rules assume the host copy pipeline owns those chords, and under aterm
    // that pipeline IS this chord (canvas selections have no DOM selection for
    // a native copy event to pick up).
    if (tryCopyChord(event)) {
      event.preventDefault()
      return
    }
    // xterm's attachCustomKeyEventHandler contract: consult the consumer before
    // any encoding; `false` = handled/suppressed upstream (interrupt, IME, JIS-
    // yen, clipboard bypass) — send nothing and leave the browser default alone
    // (paste/native events may still need to fire).
    const customKeyEventHandler = getCustomKeyEventHandler?.()
    if (customKeyEventHandler && customKeyEventHandler(event) === false) {
      return
    }
    if (tryScrollbackPage(event)) {
      event.preventDefault()
      return
    }
    if (term.is_alt_screen && (event.key === 'PageUp' || event.key === 'PageDown')) {
      if (shouldNoteAtermMatrixRainActivity(effectsTerm)) {
        effectsTerm.note_matrix_rain_alt_scroll?.()
      }
    }
    const bytes = encodeKeyEventToBytes(event, encodeWithEngine, {
      isMac,
      // Read per press so a live settings toggle applies without a pane rebuild.
      macOptionIsMeta: getMacOptionIsMeta?.() ?? false,
      getKeyboardModeBits
    })
    // Plain printable chars return null here; they flow through onInput instead,
    // so keydown sends ONLY non-text keys and nothing is double-sent. (Under
    // kitty REPORT_ALL_KEYS_AS_ESC printables DO encode here — and the
    // preventDefault below then suppresses the input event, keeping the
    // never-double-send property.)
    if (bytes === null) {
      return
    }
    event.preventDefault()
    const isUnmodifiedSubmit =
      event.key === 'Enter' && !event.shiftKey && !event.altKey && !event.ctrlKey && !event.metaKey
    noteEffectsKeystroke(isUnmodifiedSubmit)
    inputSink(bytes)
    // Predictive echo on the non-text keys this handler owns: submit ends the
    // confirmation epoch (password-prompt safety), plain Backspace cancels our own
    // trailing guess. Modified Backspace (word-delete) is left to the app's echo.
    if (isUnmodifiedSubmit) {
      predictionEcho?.noteSubmit()
    } else if (event.key === 'Backspace' && !event.ctrlKey && !event.altKey && !event.metaKey) {
      predictionEcho?.noteBackspace()
    }
    // Clear so the sink-bound textarea never accumulates the typed characters.
    textarea.value = ''
  }

  // Keyup mirrors keydown's gates (IME, consumer hook, Cmd-null, macOption) but
  // encodes with event_type=Release so kitty REPORT_EVENT_TYPES apps (neovim,
  // kitty-protocol games) see releases. The engine emits nothing for releases
  // in legacy mode, so this is free there. A keyup with no matching keydown
  // (focus gained mid-hold) is encoded anyway — the engine decides. Copy-chord
  // and scrollback paging are keydown-only host actions and are not re-run.
  const onKeyUp = (event: KeyboardEvent): void => {
    if (event.isComposing || composing) {
      return
    }
    // Same consumer contract as keydown; the hook is keyup-aware (it suppresses
    // e.g. the keyup paired with a host-handled interrupt press).
    const customKeyEventHandler = getCustomKeyEventHandler?.()
    if (customKeyEventHandler && customKeyEventHandler(event) === false) {
      return
    }
    const bytes = encodeKeyEventToBytes(event, encodeWithEngine, {
      isMac,
      macOptionIsMeta: getMacOptionIsMeta?.() ?? false,
      getKeyboardModeBits
    })
    if (bytes === null) {
      return
    }
    // preventDefault only when bytes were actually sent (legacy releases are
    // null), so app/browser keyup behavior is untouched outside kitty mode.
    event.preventDefault()
    inputSink(bytes)
  }

  // Text input path (typing, paste via setRangeText+InputEvent, IME commit): the
  // helper textarea has no keydown sender for printable chars (see onKeyDown), so
  // the actual character bytes arrive here. Mirrors xterm: keydown = non-text
  // keys, input/compositionend = text.
  const onInput = (event: Event): void => {
    const inputEvent = event as InputEvent
    // A programmatic paste (text-control-paste.ts) fires an InputEvent with
    // inputType 'insertFromPaste'/'insertReplacementText' and isComposing=false
    // even while a local IME composition is open; it must still reach the PTY, so
    // let paste/replacement inputs through regardless of the composing flag.
    const isPasteInsert =
      inputEvent.inputType === 'insertFromPaste' || inputEvent.inputType === 'insertReplacementText'
    // Otherwise compositionend handles the committed IME string; ignore inputs
    // fired while composing (local flag OR event.isComposing) so a composed run
    // isn't sent twice char-by-char.
    if (!isPasteInsert && (composing || inputEvent.isComposing)) {
      return
    }
    // For insertText/insertFromPaste InputEvent.data carries the inserted text.
    // Chunked/large pastes (text-control-paste.ts) and some browsers fire an
    // InputEvent with null data after mutating value — read textarea.value then.
    const data = inputEvent.data ?? textarea.value
    if (data) {
      // Route pastes through pasteSink so DECSET 2004 wraps them with the
      // bracketed-paste markers; typing/IME stays raw via inputSink. A null-data
      // InputEvent only reads textarea.value for a paste (isPasteInsert), so the
      // value-fallback path is always a paste and belongs on the paste sink.
      if (isPasteInsert) {
        pasteSink(data)
      } else {
        noteEffectsKeystroke()
        // Predictive echo: track each typed printable as a speculative ghost (the
        // engine declines non-printables/wraps itself). Skipped for paste (bulk,
        // not per-key echo) and IME (committed via compositionend, not here).
        if (predictionEcho) {
          for (const ch of data) {
            predictionEcho.noteChar(ch)
          }
        }
        inputSink(data)
      }
    }
    // Always clear so the sink-bound textarea never accumulates sent characters.
    textarea.value = ''
  }

  // IME: buffer composing keystrokes, then send the committed string on end.
  // The composition view anchors the textarea + paints the preedit at the cursor
  // cell for the duration; it never sends anything.
  const onCompositionStart = (): void => {
    composing = true
    compositionView.begin()
  }
  // compositionupdate only RENDERS the in-progress string (sending it would
  // double-send what compositionend commits).
  const onCompositionUpdate = (event: CompositionEvent): void => {
    compositionView.update(event.data ?? '')
  }
  const onCompositionEnd = (event: CompositionEvent): void => {
    composing = false
    // Committed text sends exactly once, here (a cancel delivers empty data).
    if (event.data) {
      noteEffectsKeystroke()
      inputSink(event.data)
    }
    textarea.value = ''
    compositionView.end()
  }

  textarea.addEventListener('keydown', onKeyDown)
  textarea.addEventListener('keyup', onKeyUp)
  textarea.addEventListener('input', onInput)
  textarea.addEventListener('compositionstart', onCompositionStart)
  textarea.addEventListener('compositionupdate', onCompositionUpdate)
  textarea.addEventListener('compositionend', onCompositionEnd)

  return {
    dispose: () => {
      textarea.removeEventListener('keydown', onKeyDown)
      textarea.removeEventListener('keyup', onKeyUp)
      textarea.removeEventListener('input', onInput)
      textarea.removeEventListener('compositionstart', onCompositionStart)
      textarea.removeEventListener('compositionupdate', onCompositionUpdate)
      textarea.removeEventListener('compositionend', onCompositionEnd)
      compositionView.dispose()
    }
  }
}
