import type { ManagedPaneInternal } from '../pane-manager-types'
import { useAppStore } from '@/store'
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
      getCopyOnSelect: () => useAppStore.getState().settings?.terminalClipboardOnSelect === true,
      getCursorBlink: () => pane.terminal.options.cursorBlink !== false,
      // Honor the user's terminalFontSize (UI clamps 10–24) instead of a hardcoded
      // 14px; read live so a size change re-rasterizes via the grid reflow.
      getFontPx: () => {
        const size = useAppStore.getState().settings?.terminalFontSize
        return typeof size === 'number' && size > 0 ? size : ATERM_RENDERER_FONT_PX
      }
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
