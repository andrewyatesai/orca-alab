import type { ManagedPaneInternal } from '../pane-manager-types'
import { useAppStore } from '@/store'
import { resolveTerminalLigaturesEnabled } from '../../../../../shared/terminal-ligatures'
import {
  normalizeTerminalFastScrollSensitivity,
  normalizeTerminalScrollSensitivity
} from '../pane-terminal-options'
import { normalizeTerminalTuiMouseWheelMultiplier } from '../pane-terminal-mouse-wheel'
import { normalizeDesktopTerminalScrollbackRows } from '../../../../../shared/terminal-scrollback-policy'
import { createAtermPaneController, type AtermLinkContext } from './aterm-pane-renderer'
import { ATERM_RENDERER_FONT_PX } from './aterm-pane-controller-types'

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
  void createAtermPaneController(
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
      // Honor the user's terminalLineHeight (UI clamps 1–3); read live so a change
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
      formatLinkTooltip: (url, hint) => pane.formatLinkTooltip?.(url, hint)
    }
  )
    .then((controller) => {
      // If the pane was torn down while wasm/font loaded, drop the controller.
      if (pane.disposed) {
        controller.dispose()
        return
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
    })
    .catch((err) => {
      // Async wasm/font failure: there is no xterm fallback renderer anymore, so
      // log it. The facade keeps buffering output until (if ever) a controller
      // attaches; the pane self-heals if a later retry succeeds.
      console.error('[aterm] failed to create canvas renderer for pane', pane.id, err)
    })
}
