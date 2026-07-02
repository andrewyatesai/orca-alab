// Per-pane state + command dispatch for the SHARED render worker: the worker entry
// (aterm-render-worker) owns the paneId→PaneRuntime registry and the lifecycle
// messages (init / fallback / dispose); every other pane-scoped command lands here.
// Split from the entry to keep both files under the line budget.

import type { StoredInit } from './aterm-worker-engine-build'
import type { WorkerTerminal } from './aterm-worker-terminal'
import type { WorkerFrameScheduler } from './aterm-worker-frame-scheduler'
import type {
  AtermWorkerPaneEvent,
  AtermWorkerPaneRuntimeCommand
} from './aterm-render-worker-protocol'

// Both engine bindings ship these (aterm_wasm/aterm_gpu_web), but the WorkerEngine
// Pick + worker terminal predate them — cast here, surgically, until the planned
// worker refactor folds them into aterm-worker-terminal.
export type EngineSettingSetters = {
  set_minimum_contrast: (ratio: number) => void
  set_word_separators: (separators?: string | null) => void
  set_background_opacity: (opacity: number) => void
  set_cursor_opacity: (opacity: number) => void
  set_kitty_keyboard_enabled: (enabled: boolean) => void
}

/** Everything the worker keeps for ONE pane. Deliberately per-pane: the serialize
 *  cache, frame scheduler (dirty flag + suspend state) and stored init must never be
 *  shared, so one pane's dispose/crash-seed/suspend can't touch another's. */
export type PaneRuntime = {
  paneId: number
  term: WorkerTerminal | null
  engineSetters: EngineSettingSetters | null
  storedInit: StoredInit | null
  canvas: OffscreenCanvas | null
  /** Don't fall back twice if more than one init error is posted for this pane. */
  fellBackToCpu: boolean
  /** Set by 'dispose' — an engine still building when it flips is freed on arrival. */
  disposed: boolean
  frameScheduler: WorkerFrameScheduler
  serializeCache: { schedule: () => void; dispose: () => void }
  /** Post a pane event to main; the entry stamps this pane's paneId on it. */
  post: (event: AtermWorkerPaneEvent) => void
}

/** Handle one non-lifecycle command for `pane`. Commands for a pane whose engine is
 *  still building (or died) no-op safely, same as the single-engine worker did. */
export function dispatchPaneCommand(pane: PaneRuntime, msg: AtermWorkerPaneRuntimeCommand): void {
  const term = pane.term
  const scheduleDraw = pane.frameScheduler.schedule
  switch (msg.type) {
    case 'process': {
      if (!term) {
        return
      }
      const side = term.processBytes(msg.data)
      // Post the edge-triggered side channels immediately (NOT coalesced) so none are
      // dropped: replies → PTY, OSC app-events → dispatch, bell → re-emit.
      if (side.reply) {
        pane.post({ type: 'reply', data: side.reply })
      }
      if (side.osc) {
        pane.post({ type: 'osc', events: side.osc })
      }
      if (side.bell) {
        pane.post({ type: 'bell' })
      }
      scheduleDraw()
      pane.serializeCache.schedule()
      return
    }
    case 'draw':
      scheduleDraw()
      return
    case 'resize':
      term?.resize(msg.rows, msg.cols)
      scheduleDraw()
      return
    case 'setPx':
      term?.setPx(msg.px)
      scheduleDraw()
      return
    case 'setLineHeight':
      term?.setLineHeight(msg.lineHeight)
      scheduleDraw()
      return
    case 'setLigatures':
      term?.setLigatures(msg.on)
      scheduleDraw()
      return
    case 'setScrollbackLimit':
      term?.setScrollbackLimit(msg.lines)
      return
    case 'setMinimumContrast':
      // Appearance-only: repaint so the floored fg shows without waiting for output.
      pane.engineSetters?.set_minimum_contrast(msg.ratio)
      scheduleDraw()
      return
    case 'setWordSeparators':
      // Selection-behavior only (next double-click) — no repaint needed.
      pane.engineSetters?.set_word_separators(msg.separators ?? undefined)
      return
    case 'setBackgroundOpacity':
      // Appearance-only: repaint so the translucent default bg shows immediately.
      pane.engineSetters?.set_background_opacity(msg.opacity)
      scheduleDraw()
      return
    case 'setCursorOpacity':
      pane.engineSetters?.set_cursor_opacity(msg.opacity)
      scheduleDraw()
      return
    case 'setKittyKeyboardEnabled':
      // Protocol capability only (affects future CSI ? u replies) — no repaint.
      pane.engineSetters?.set_kitty_keyboard_enabled(msg.enabled)
      return
    case 'setDefaultCursorStyle':
      term?.setDefaultCursorStyle(msg.param)
      scheduleDraw()
      return
    case 'setColorScheme': {
      // set_color_scheme may queue a CSI ?997 push (when the scheme changed AND the app
      // enabled DEC 2031); forward it through the reply channel → main → PTY.
      const reply = term ? term.setColorScheme(msg.dark) : ''
      if (reply) {
        pane.post({ type: 'reply', data: reply })
      }
      return
    }
    case 'scrollLines':
      term?.scrollLines(msg.delta)
      scheduleDraw()
      return
    case 'scrollToBottom':
      term?.scrollToBottom()
      scheduleDraw()
      return
    case 'scrollToTop':
      term?.scrollToTop()
      scheduleDraw()
      return
    case 'scrollToLine':
      term?.scrollToLine(msg.line)
      scheduleDraw()
      return
    case 'selectionStart':
      term?.selectionStart(msg.row, msg.col)
      scheduleDraw()
      return
    case 'selectionExtend':
      term?.selectionExtend(msg.row, msg.col)
      scheduleDraw()
      return
    case 'selectionFinish':
      term?.selectionFinish()
      scheduleDraw()
      return
    case 'selectionWord':
      term?.selectionWord(msg.row, msg.col)
      scheduleDraw()
      return
    case 'selectionLine':
      term?.selectionLine(msg.row, msg.col)
      scheduleDraw()
      return
    case 'selectionClear':
      term?.selectionClear()
      scheduleDraw()
      return
    case 'themeSet':
      term?.themeSet(msg)
      scheduleDraw()
      return
    case 'setSelectionInactive':
      term?.setSelectionInactive(msg.inactive)
      scheduleDraw()
      return
    case 'setSelectionInactiveBg':
      term?.setSelectionInactiveBg(msg.bg)
      scheduleDraw()
      return
    case 'setClipboardWriteAuthorized':
      term?.setClipboardWriteAuthorized(msg.allowed)
      return
    case 'setDrawSuspended':
      pane.frameScheduler.setSuspended(msg.suspended)
      return
    case 'setCursorBlinkPhase':
      // Render-only: repaint the cursor cell, but post NO state (no snapshot field tracks
      // blink phase, so the STATE would be byte-identical).
      term?.setCursorBlinkPhase(msg.on)
      scheduleDraw(false)
      return
    case 'setCursorHollow':
      term?.setCursorHollow(msg.hollow)
      scheduleDraw(false)
      return
    case 'setHover':
      term?.setHover('clear' in msg ? null : { row: msg.row, col: msg.col })
      scheduleDraw()
      return
    case 'searchFind':
      term?.searchFind(msg.query, msg.caseSensitive, msg.isRegex)
      scheduleDraw()
      return
    case 'searchNext':
      term?.searchNext()
      scheduleDraw()
      return
    case 'searchPrev':
      term?.searchPrev()
      scheduleDraw()
      return
    case 'searchClear':
      term?.searchClear()
      scheduleDraw()
      return
    case 'setPrimaryFont':
      term?.setPrimaryFont(msg.bytes)
      scheduleDraw()
      return
    case 'setBoldFont':
      term?.setBoldFont(msg.bytes)
      scheduleDraw()
      return
    case 'mouseEncode': {
      // The encoded mouse report is PTY input — forward it through the reply channel
      // (→ main onReply → inputSink), same as engine query replies.
      const data = term
        ? term.mouseEncode(msg.kind, msg.col, msg.row, msg.button, msg.mods, msg.up ?? false)
        : ''
      if (data) {
        pane.post({ type: 'reply', data })
      }
      return
    }
    case 'query': {
      // 'flush' is a parse fence, not an engine read: reaching it means every
      // earlier message (process bytes + their posted replies) was handled, so
      // answer directly — even with no engine yet.
      const value =
        msg.kind === 'flush' ? true : term ? term.query(msg.kind, msg.arg, msg.arg2) : null
      pane.post({ type: 'queryResult', id: msg.id, value })
    }
  }
}
