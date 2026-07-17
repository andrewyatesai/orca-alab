import type { IDisposable, ILinkProvider } from './terminal-types'
import type { AtermPaneController } from './aterm-pane-controller-types'
import { createAtermFacadeBuffer } from './aterm-facade-buffer'
import { createAtermFacadeParser } from './aterm-facade-parser'
import { createFacadeEmitter } from './aterm-facade-emitters'
import { parseAtermNotifications, type AtermAppNotification } from './aterm-notification-drain'
import type { AtermFacadeOptions, AtermTerminalFacade } from './aterm-terminal-facade-types'

// The facade's public types (AtermTerminalFacade/AtermFacadeOptions) live in the
// sibling types file to keep this implementation under the line cap; re-export so
// the ~46 consumers importing them from this module keep resolving unchanged.
export type { AtermFacadeOptions, AtermTerminalFacade } from './aterm-terminal-facade-types'

const ESC = '\u001b' // ESC (CSI/OSC introducer).
const ESC_SYMBOL = '\u241b' // SYMBOL FOR ESCAPE — replaces embedded ESC in pastes.
const BRACKETED_PASTE_START = '\u001b[200~'
const BRACKETED_PASTE_END = '\u001b[201~'
// Clear screen + scrollback + home cursor (xterm clear() equivalent).
const CLEAR_SCREEN_AND_SCROLLBACK = '\u001b[2J\u001b[3J\u001b[H'

type AtermFacadeDeps = {
  /** Initial options (theme/font/cursor/etc.) the controller reads live. */
  options: AtermFacadeOptions
}

export function createAtermTerminalFacade(deps: AtermFacadeDeps): AtermTerminalFacade {
  let controller: AtermPaneController | null = null
  // The aterm DOM nodes (mirror xterm's element/textarea). Late-bound on attach;
  // undefined until then (consumers guard with ?./??, matching xterm's
  // unopened-terminal behavior the facade replaces).
  let element: HTMLElement | undefined
  let textarea: HTMLTextAreaElement | undefined
  // PTY/replay bytes that arrive before the async controller attaches. Buffered
  // in order and flushed on attach so no process() byte is dropped or reordered.
  const preAttachBuffer: string[] = []
  let disposed = false

  const dataEmitter = createFacadeEmitter<string>()
  const resizeEmitter = createFacadeEmitter<{ cols: number; rows: number }>()
  const bellEmitter = createFacadeEmitter<void>()
  const appNotificationEmitter = createFacadeEmitter<AtermAppNotification>()
  const selectionChangeEmitter = createFacadeEmitter<void>()
  const titleListeners = new Set<(title: string) => void>()
  let titleDisposable: IDisposable | null = null
  let lastSelectionSignature = ''
  // Consumer-registered link providers (term_/task_ handles, cwd-resolved file
  // paths). The controller's link input consults them where the engine reports
  // no link; registration order is xterm's provider precedence.
  const linkProviders: ILinkProvider[] = []
  // The consumer's keyboard hook (attachCustomKeyEventHandler); read per keydown
  // by the textarea input via the controller options.
  let customKeyEventHandler: ((event: KeyboardEvent) => boolean) | null = null
  // A resize() issued before the async controller attach (snapshot replay can
  // race pane creation); applied on attach BEFORE the buffered bytes replay.
  let pendingResize: { cols: number; rows: number } | null = null

  const { buffer, registerMarker, pollBufferChange } = createAtermFacadeBuffer(() => controller)
  const { parser, dispatchOscEvent } = createAtermFacadeParser()

  // After each engine process(), drain the engine's edge-triggered side channels
  // (BEL, OSC app-events) and notify selection-change subscribers if the range
  // moved. Replies (take_response) are drained by the wiring's own process().
  const drainEngineSideChannels = (): void => {
    if (!controller) {
      return
    }
    // Skip the per-chunk bell drain entirely when nothing listens: the live bell
    // UX is served by the TS bell detector (pty-transport), not this facade
    // channel, so draining here is a wasted wasm crossing on every output chunk.
    // onBell() clears any flag accrued while unlistened so a late subscriber never
    // replays a stale phantom bell.
    if (bellEmitter.hasListeners() && controller.drainBell()) {
      bellEmitter.emit()
    }
    const osc = controller.takeOscEvents()
    if (osc) {
      let events: [number, string][] = []
      try {
        events = JSON.parse(osc)
      } catch {
        events = []
      }
      for (const [code, value] of events) {
        // dispatchOscEvent re-encodes the engine-decoded payload to the xterm wire
        // format the unchanged orca OSC handlers parse.
        dispatchOscEvent(code, value)
      }
    }
    // OSC 9/99/777 desktop notifications ride the same post-process drain; skip the
    // per-chunk engine crossing while nothing subscribed (mirrors the bell gate).
    if (appNotificationEmitter.hasListeners()) {
      for (const notification of parseAtermNotifications(controller.takeNotifications())) {
        appNotificationEmitter.emit(notification)
      }
    }
    pollBufferChange()
    maybeEmitSelectionChange()
  }

  const maybeEmitSelectionChange = (): void => {
    if (!controller || !selectionChangeEmitter.hasListeners()) {
      return
    }
    const range = controller.selectionRange()
    const signature = range ? `${range.startX},${range.startY},${range.endX},${range.endY}` : ''
    if (signature !== lastSelectionSignature) {
      lastSelectionSignature = signature
      selectionChangeEmitter.emit()
    }
  }

  const facade: AtermTerminalFacade = {
    get isDisposed() {
      return disposed
    },
    get cols() {
      return controller?.gridSize().cols ?? 0
    },
    get rows() {
      return controller?.gridSize().rows ?? 0
    },
    buffer,
    parser,
    get element() {
      return element
    },
    get textarea() {
      return textarea
    },
    options: deps.options,
    // aterm bakes Unicode 11 width tables into the engine; activeVersion is a real
    // constant. versions/register are documented no-ops (engine owns widths).
    unicode: {
      activeVersion: '11',
      versions: ['11'],
      register: () => undefined
    },
    // Live DEC mode reads off the engine (no separate modes object exists).
    modes: {
      get applicationCursorKeysMode() {
        return controller?.isAppCursorMode() ?? false
      },
      get bracketedPasteMode() {
        return controller?.bracketedPasteMode() ?? false
      },
      get mouseTrackingMode() {
        // Only the !== 'none' distinction is consumed (TerminalPane mouse routing),
        // so map the engine's live tracking flag to xterm's 'vt200'/'none'.
        return controller?.isMouseTracking() ? 'vt200' : 'none'
      },
      get sendFocusMode() {
        return controller?.isFocusEventMode() ?? false
      }
    },
    write(data, callback) {
      // Direct callers (e.g. RESET_KITTY, settings preview, e2e shim writes that
      // inject control sequences) feed the engine here — they DON'T go through the
      // scheduler's up-front mirror, so this is their only path to the engine.
      // Scheduler output uses __schedulerWrite (mirror already fed it). Empty data
      // is just a parse-settle ping, so skip the feed and only fire the callback.
      if (data) {
        feedEngine(data)
      }
      callback?.()
    },
    __schedulerWrite(_data, callback) {
      // The output scheduler's delayed/coalesced write is NOT the engine feed:
      // bytes reach the engine up-front and in order via __feedEngine (mirror
      // OutputToAterm). Here we only fire the parsed callback the foreground
      // settle/await machinery depends on, so we never double-process bytes.
      callback?.()
    },
    __scheduleAtermDraw() {
      // The mirror fed the engine up front; a callback-only __schedulerWrite paints
      // nothing, so the scheduler asks for a draw here to flush the engine's latest
      // state to the canvas. No-op until the controller attaches (the attach replay
      // schedules its own draw via the pump).
      if (!disposed) {
        controller?.scheduleDraw()
      }
    },
    input(data) {
      // User input → the same PTY pipeline xterm's onData fed. The pty-connection
      // onData handler (routePtyInputData) subscribes via onData, so emitting here
      // routes through its intent/presence/replay guards.
      dataEmitter.emit(data)
    },
    paste(text) {
      // Mirror xterm.paste: normalize \r?\n→\r, and in bracketed-paste mode
      // (DECSET 2004) wrap in ESC[200~..ESC[201~ with embedded ESC neutralized
      // (paste-injection guard), then route through the PTY pipeline via input().
      const normalized = text.replace(/\r?\n/g, '\r')
      // Honor xterm's ignoreBracketedPasteMode override (the interrupted-paste
      // path sets it to force an unbracketed paste).
      const bracketed =
        deps.options.ignoreBracketedPasteMode !== true &&
        (controller?.bracketedPasteMode() ?? false)
      // Neutralize embedded ESC (U+001B → U+241B) so a pasted ESC[201~ can't end
      // the bracket early (paste-injection guard). split/join avoids a control char
      // in a regex literal (oxlint no-control-regex).
      const guarded = normalized.split(ESC).join(ESC_SYMBOL)
      const data = bracketed
        ? `${BRACKETED_PASTE_START}${guarded}${BRACKETED_PASTE_END}`
        : normalized
      dataEmitter.emit(data)
    },
    resize(cols, rows) {
      // Real grid resize with an override the container observer honors until
      // safeFit's fitToContainer clears it (snapshot replay at source dims,
      // mobile-fit hold). The controller's commit reports the new grid through
      // resizeSink → routePtyResize synchronously, so the snapshot-replay
      // suppression flag still gates the PTY resize (xterm's onResize timing).
      if (controller) {
        controller.resize(cols, rows)
        return
      }
      // Pre-attach there is no grid yet: remember the dims for the attach (the
      // buffered replay must land in them) and keep the PTY-notify contract.
      pendingResize = { cols, rows }
      resizeEmitter.emit({ cols, rows })
    },
    clear() {
      // xterm clear() wipes screen + scrollback and homes the cursor.
      controller?.process(CLEAR_SCREEN_AND_SCROLLBACK)
    },
    // Documented no-ops: aterm has no reset()/refresh() — the engine auto-renders
    // on its own draw scheduler, and reset is unused in production (contract).
    reset() {
      /* no-op: aterm owns rendering; reset is unused by orca (contract). */
    },
    refresh() {
      /* no-op: aterm auto-refreshes via its draw scheduler (contract). */
    },
    focus() {
      textarea?.focus()
    },
    blur() {
      textarea?.blur()
    },
    scrollToBottom() {
      controller?.scrollToBottom()
    },
    scrollToTop() {
      controller?.scrollToTop()
    },
    scrollToLine(line) {
      controller?.scrollToLine(line)
    },
    scrollLines(amount) {
      // xterm scrollLines: positive = toward newer/bottom. aterm scroll_lines:
      // positive = older/up. Invert so xterm consumers scroll the expected way.
      controller?.scrollLines(-amount)
    },
    registerMarker(cursorYOffset) {
      return registerMarker(cursorYOffset ?? 0)
    },
    loadAddon() {
      /* no-op: aterm panes don't load xterm addons; the controller owns search/
       * serialize/links/unicode natively (contract). */
    },
    attachCustomKeyEventHandler(handler) {
      // xterm keeps ONE handler (a re-attach replaces it); the textarea keydown
      // path consults it before encoding via the controller options.
      customKeyEventHandler = handler
    },
    get __customKeyEventHandler() {
      return customKeyEventHandler
    },
    registerLinkProvider(provider) {
      // Registration order is the hit-test precedence (xterm's linkifier); the
      // controller's link input consults these where the engine's native
      // detection (link_at) reports no link.
      linkProviders.push(provider)
      return {
        dispose: () => {
          const index = linkProviders.indexOf(provider)
          if (index !== -1) {
            linkProviders.splice(index, 1)
          }
        }
      }
    },
    getSelection() {
      return controller?.selectionText() ?? ''
    },
    hasSelection() {
      return (controller?.selectionText() ?? '') !== ''
    },
    clearSelection() {
      controller?.clearSelection()
      maybeEmitSelectionChange()
    },
    // Pre-attach there is no hover state yet — nothing to invalidate.
    resetLinkHoverCache: () => void controller?.resetLinkHoverCache(),
    getSelectionPosition() {
      const range = controller?.selectionRange()
      if (!range) {
        return null
      }
      return {
        start: { x: range.startX, y: range.startY },
        end: { x: range.endX, y: range.endY }
      }
    },
    onData(handler) {
      // aterm input bypasses this via pane.routePtyInput (same pipeline), so this
      // emitter is the honest registration point but stays dormant for aterm.
      return dataEmitter.on(handler)
    },
    onResize(handler) {
      return resizeEmitter.on(handler)
    },
    onTitleChange(handler) {
      titleListeners.add(handler)
      // Bind to the controller's title channel lazily (it may not be attached yet).
      bindTitleSourceIfReady()
      return { dispose: () => void titleListeners.delete(handler) }
    },
    onBell(handler) {
      // First subscriber: clear any BEL accrued while the drain was skipped
      // (unlistened), so the next per-chunk drain can't replay a stale phantom bell.
      if (controller && !bellEmitter.hasListeners()) {
        controller.drainBell()
      }
      return bellEmitter.on(handler)
    },
    onSelectionChange(handler) {
      return selectionChangeEmitter.on(handler)
    },
    onTerminalAppNotification(handler) {
      return appNotificationEmitter.on(handler)
    },
    dispose() {
      disposed = true
      titleDisposable?.dispose()
      titleDisposable = null
      titleListeners.clear()
      dataEmitter.clear()
      resizeEmitter.clear()
      bellEmitter.clear()
      appNotificationEmitter.clear()
      selectionChangeEmitter.clear()
      linkProviders.length = 0
      customKeyEventHandler = null
      controller?.dispose()
      controller = null
    },
    __attachController(next, dom) {
      if (disposed) {
        next.dispose()
        return
      }
      controller = next
      element = dom.element
      textarea = dom.textarea
      bindTitleSourceIfReady()
      // Worker path: the engine replies in a later task than the posted process(), so
      // drain the moment the worker pushes OSC/bell rather than waiting for the next
      // chunk (the prompt's final-chunk events would otherwise lag or be lost on idle
      // close). In-process leaves this unset → the synchronous post-process drain stands.
      controller.onEngineSideChannel?.(() => drainEngineSideChannels())
      // Mouse selection must ping onSelectionChange itself (PRIMARY/copy-on-select
      // on idle shells); the drain-side dedupe keeps output-driven emits single.
      controller.onSelectionMutation(() => maybeEmitSelectionChange())
      controller.setLinkProviderSource(() => linkProviders)
      // A pre-attach resize (snapshot replay racing pane creation) must size the
      // grid BEFORE the buffered bytes replay into it.
      if (pendingResize) {
        controller.resize(pendingResize.cols, pendingResize.rows)
        pendingResize = null
      }
      // Replay buffered pre-attach output IN ORDER, then drain side channels.
      const buffered = preAttachBuffer.splice(0, preAttachBuffer.length)
      for (const chunk of buffered) {
        controller.process(chunk)
      }
      drainEngineSideChannels()
    },
    __feedEngine(data) {
      feedEngine(data)
    }
  }

  // Subscribe the facade's title listeners to the controller's onTitleChange the
  // moment the controller exists (handlers can register before attach).
  function bindTitleSourceIfReady(): void {
    if (titleDisposable || !controller || titleListeners.size === 0) {
      return
    }
    titleDisposable = controller.onTitleChange((title) => {
      titleListeners.forEach((listener) => listener(title))
    })
  }

  // The engine-feed entry point (called by mirrorOutputToAterm via __feedEngine).
  // Buffers until the controller attaches; after that, processes live and drains
  // side channels.
  function feedEngine(data: string): void {
    if (disposed) {
      return
    }
    if (!controller) {
      preAttachBuffer.push(data)
      return
    }
    controller.process(data)
    drainEngineSideChannels()
  }

  return facade
}
