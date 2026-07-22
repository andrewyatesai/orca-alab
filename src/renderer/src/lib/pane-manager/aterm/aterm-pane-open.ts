import type { ManagedPaneInternal } from '../pane-manager-types'
import { useAppStore } from '@/store'
import { resolveTerminalLigaturesEnabled } from '../../../../../shared/terminal-ligatures'
import {
  normalizeTerminalFastScrollSensitivity,
  normalizeTerminalScrollSensitivity
} from '../pane-terminal-options'
import { normalizeTerminalTuiMouseWheelMultiplier } from '../pane-terminal-mouse-wheel'
import { normalizeDesktopTerminalScrollbackRows } from '../../../../../shared/terminal-scrollback-policy'
import { resolveCursorAgentImeAnchor } from '../terminal-ime-anchor'
import { createAtermPaneController, type AtermLinkContext } from './aterm-pane-renderer'
import { ATERM_RENDERER_FONT_PX } from './aterm-pane-controller-types'
import { flushPendingAtermRainPulsesAtControllerAttach } from './aterm-rain-pulse-delivery'
import { makePaneKey } from '../../../../../shared/stable-pane-id'
import { getRegisteredTabIdsForController } from '../pane-manager-registry'
import { engineColorToCss } from '../../terminal-themes/engine-color-css'

// Session restore opens many panes in one tick, and CONCURRENT engine builds
// contend (wasm compile + font parse + GL acquire) — stretching every pane's
// first frame toward the worker's 15s deadline (see aterm-worker-loader).
// Admit a few builds at a time so the first panes paint fast and the rest
// follow; queued panes are never blank (their container carries the theme
// background from createPaneDOM) and their facades keep buffering PTY output.
const MAX_CONCURRENT_PANE_BUILDS = 2
// Safety valve: a wedged build (hung asset fetch) must not dam the queue for
// every later pane — a waiter self-admits past the limit after this long.
const PANE_BUILD_ADMIT_FALLBACK_MS = 20_000

export type AtermPaneBuildQueue = {
  admit: () => Promise<void>
  release: () => void
}

/** FIFO admission gate for pane engine builds (factory exported for tests). */
export function createAtermPaneBuildQueue(limit: number): AtermPaneBuildQueue {
  let inFlight = 0
  const waiting: (() => void)[] = []
  return {
    admit: (): Promise<void> => {
      if (inFlight < limit) {
        inFlight++
        return Promise.resolve()
      }
      return new Promise((resolve) => {
        const entry = (): void => {
          clearTimeout(fallback)
          resolve()
        }
        const fallback = setTimeout(() => {
          const index = waiting.indexOf(entry)
          if (index >= 0) {
            // Self-admit past the limit rather than wait on a wedged build; the
            // matching release() keeps the count consistent either way.
            waiting.splice(index, 1)
            inFlight++
            resolve()
          }
        }, PANE_BUILD_ADMIT_FALLBACK_MS)
        waiting.push(entry)
      })
    },
    release: (): void => {
      const next = waiting.shift()
      if (next) {
        // Hand the slot straight to the next queued build; inFlight unchanged.
        next()
        return
      }
      inFlight = Math.max(0, inFlight - 1)
    }
  }
}

const paneBuildQueue = createAtermPaneBuildQueue(MAX_CONCURRENT_PANE_BUILDS)

// In-flight cue past ~300ms (STYLEGUIDE: defer visible loading feedback so fast
// local opens see nothing, slow builds/queue waits show a busy cursor).
export const ATERM_PANE_BUILD_CUE_DELAY_MS = 300

/** Build the aterm canvas controller over the pane's xterm container and bind it
 *  into the pane's facade terminal. Creation is async (wasm + font load); a pane
 *  disposed before the controller resolves drops it instead of attaching a leaked
 *  canvas.
 *
 *  Input/resize route through pane.routePtyInput / pane.routePtyResize — the exact
 *  PTY seam connectPanePty wires (intent tracking, presence locks, replay guards)
 *  — bypassing the facade. Pre-connect (before those are set) the bytes are
 *  dropped, matching the prior pre-connect behavior (no PTY exists yet).
 *
 *  linkContext (optional) routes terminal URL clicks through orca's opener so the
 *  in-app/system-browser preference is honored; omitted, URLs open in the system
 *  browser. */
export function openAtermPane(pane: ManagedPaneInternal, linkContext?: AtermLinkContext): void {
  void openAtermPaneAdmitted(pane, linkContext)
}

async function openAtermPaneAdmitted(
  pane: ManagedPaneInternal,
  linkContext?: AtermLinkContext
): Promise<void> {
  // Busy cursor only past the deferral window, so a warm open shows nothing.
  const cueTimer = setTimeout(() => {
    pane.container.style.cursor = 'progress'
  }, ATERM_PANE_BUILD_CUE_DELAY_MS)
  const clearCue = (): void => {
    clearTimeout(cueTimer)
    pane.container.style.cursor = ''
  }
  await paneBuildQueue.admit()
  if (pane.disposed) {
    clearCue()
    paneBuildQueue.release()
    return
  }
  try {
    const controller = await buildAtermPaneController(pane, linkContext)
    // If the pane was torn down while wasm/font loaded, drop the controller.
    if (pane.disposed) {
      controller.dispose()
      return
    }
    // Keep the container's never-blank paint (createPaneDOM) in lockstep with
    // live re-themes, so the DOM behind a translucent or resizing canvas never
    // shows a stale background color.
    const engineUpdateTheme = controller.updateTheme
    controller.updateTheme = (colors) => {
      pane.container.style.background = engineColorToCss(colors.bg)
      engineUpdateTheme(colors)
    }
    pane.atermController = controller
    // A pane created while its manager was rendering-suspended starts paused so
    // a hidden/background manager paints no frames until resumeRendering().
    if (pane.startRenderingSuspended) {
      controller.setDrawSuspended(true)
    }
    // Bind the controller (+ its DOM) into the facade and flush buffered output.
    pane.terminal.__attachController(controller, {
      element: controller.element,
      textarea: controller.textarea
    })
    // Hook IPC can beat the async wasm/font build. Flush its bounded,
    // payload-free latch at the exact point this pane becomes drivable.
    flushPendingAtermRainPulsesAtControllerAttach(pane.leafId, controller)
    // Cross-pane spill identity: the durable tab id is only resolvable at this
    // attach edge (same registry-identity walk the rain flush uses). makePaneKey
    // throws on non-UUID leaf ids — such panes simply stay off the overlay.
    const [spillTabId] = getRegisteredTabIdsForController(pane.leafId, controller)
    if (spillTabId) {
      try {
        controller.bindSpillPaneKey(makePaneKey(spillTabId, pane.leafId))
      } catch {
        /* ignore */
      }
    }
  } catch (err) {
    // Async wasm/font failure: there is no xterm fallback renderer anymore, so
    // log it. The facade keeps buffering output until (if ever) a controller
    // attaches; the pane self-heals if a later retry succeeds.
    console.error('[aterm] failed to create canvas renderer for pane', pane.id, err)
  } finally {
    clearCue()
    paneBuildQueue.release()
  }
}

function buildAtermPaneController(
  pane: ManagedPaneInternal,
  linkContext?: AtermLinkContext
): ReturnType<typeof createAtermPaneController> {
  return createAtermPaneController(
    pane.xtermContainer,
    // Keystrokes/drained-replies → the PTY pipeline once wired; dropped pre-connect.
    (data) => pane.routePtyInput?.(data),
    // Resize → the PTY pipeline once wired; dropped pre-connect.
    (cols, rows) => pane.routePtyResize?.(cols, rows),
    // Paste: delegate to the facade's paste() which normalizes \r?\n→\r, wraps in
    // bracketed-paste markers when DECSET 2004 is on (ESC neutralized), and routes
    // through the PTY pipeline via onData. Dropped pre-connect (routePtyInput unset).
    (text) => pane.terminal.paste(text),
    linkContext,
    // Read macOptionIsMeta / cursorBlink live off the facade's options (kept in
    // sync by applyTerminalAppearance) so toggles take effect without a rebuild.
    // copy-on-select reads app settings live (default false).
    {
      getMacOptionIsMeta: () => pane.terminal.options.macOptionIsMeta === true,
      // The pane facade IS the scroll-intent target keyboard-handlers records
      // against; hand it to the input paths that scroll the engine directly
      // (Shift+PageUp/Down, scrollbar drag) and the context-loss rebuild so they
      // record/enforce intent through the same seam instead of losing the position.
      getScrollIntentTarget: () => pane.terminal,
      // The lifecycle's attachCustomKeyEventHandler hook (interrupt/IME/JIS-yen);
      // read live so it works however registration and pane-open interleave.
      getCustomKeyEventHandler: () => pane.terminal.__customKeyEventHandler,
      getCopyOnSelect: () => useAppStore.getState().settings?.terminalClipboardOnSelect === true,
      getCursorBlink: () => pane.terminal.options.cursorBlink !== false,
      // Honor the user's terminalFontSize (UI clamps 10–24) instead of a hardcoded
      // 14px; read live so a size change re-rasterizes via the grid reflow.
      getFontPx: () => {
        const size = useAppStore.getState().settings?.terminalFontSize
        return typeof size === 'number' && size > 0 ? size : ATERM_RENDERER_FONT_PX
      },
      // Honor the user's terminalLineHeight (UI clamps 1–10); read live so a change
      // re-derives the cell-box height via the grid reflow without a pane rebuild.
      getLineHeight: () => {
        const lh = useAppStore.getState().settings?.terminalLineHeight
        return typeof lh === 'number' && lh > 0 ? lh : 1
      },
      // Honor the user's terminalFontFamily: its primary face is resolved on the host
      // + injected (set_primary_font) at pane open; "JetBrains Mono"/unset = bundled.
      getFontFamily: () => useAppStore.getState().settings?.terminalFontFamily,
      // terminalFontWeight picks WHICH of the family's styles is injected (closest
      // named style; the derived bold weight selects the real set_bold_font face).
      getFontWeight: () => useAppStore.getState().settings?.terminalFontWeight,
      // Resolve terminalLigatures (auto/on/off) against the font family so set_ligatures
      // matches the user's choice; 'auto' enables only on a known-ligature face.
      getLigatures: () => {
        const settings = useAppStore.getState().settings
        return resolveTerminalLigaturesEnabled(
          settings?.terminalLigatures ?? 'auto',
          settings?.terminalFontFamily
        )
      },
      // The rows-based scrollback setting (upstream #7069 model), normalized the same
      // way the lifecycle does, so the engine retains the user's history depth.
      getScrollbackLines: () =>
        normalizeDesktopTerminalScrollbackRows(
          useAppStore.getState().settings?.terminalScrollbackRows
        ),
      // Wheel sensitivities read live off the facade's options bag (kept in sync by
      // applyTerminalAppearance) so the sliders take effect without a pane rebuild.
      getScrollSensitivity: () =>
        normalizeTerminalScrollSensitivity(pane.terminal.options.scrollSensitivity),
      getFastScrollSensitivity: () =>
        normalizeTerminalFastScrollSensitivity(pane.terminal.options.fastScrollSensitivity),
      // TUI wheel multiplier: the lifecycle threads a live settings reader onto the pane.
      getTuiScrollMultiplier: () =>
        normalizeTerminalTuiMouseWheelMultiplier(pane.terminalTuiScrollSensitivity?.()),
      // The per-pane kitty keyboard policy landed on the facade's construction options
      // (buildTerminalKeyboardProtocolOptions: vtExtensions.kittyKeyboard=false only for
      // a genuine LOCAL Windows ConPTY pane — SSH-from-Windows panes keep kitty). The
      // engine applies it once at construction (the policy is per-pane static).
      getKittyKeyboardEnabled: () => pane.terminal.options.vtExtensions?.kittyKeyboard !== false,
      // Map terminalCursorStyle + terminalCursorBlink → a DECSCUSR param: block=1/2,
      // underline=3/4, bar=5/6 (blinking/steady) — the engine's default cursor shape.
      getCursorStyleParam: () => {
        const settings = useAppStore.getState().settings
        const base =
          settings?.terminalCursorStyle === 'bar'
            ? 5
            : settings?.terminalCursorStyle === 'underline'
              ? 3
              : 1
        return settings?.terminalCursorBlink === false ? base + 1 : base
      },
      // orca's formatLinkTooltip (localhost port labels): read live off the pane
      // so the hover tooltip labels URLs however registration and pane-open
      // interleave; unset → the default "url (modifier hint)" label.
      formatLinkTooltip: (url, hint) => pane.formatLinkTooltip?.(url, hint),
      // Agent CLIs (Cursor Agent) draw their prompt while parking the real
      // cursor on a blank row — the IME anchor prefers the rendered prompt row
      // (upstream #7061), read from the facade's live buffer.
      getImeAnchor: () => {
        const buf = pane.terminal.buffer.active
        const anchor = resolveCursorAgentImeAnchor({
          buffer: buf,
          rows: pane.terminal.rows,
          cols: pane.terminal.cols,
          cursorX: buf.cursorX,
          cursorY: buf.cursorY
        })
        return anchor ? { row: anchor.row, col: anchor.column } : null
      }
    }
  )
}
