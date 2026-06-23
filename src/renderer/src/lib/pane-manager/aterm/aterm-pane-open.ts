import type { ManagedPaneInternal } from '../pane-manager-types'
import { createAtermPaneController, type AtermLinkContext } from './aterm-pane-renderer'
import { registerAtermOutputMirror } from './aterm-output-mirror'

/** Build the aterm canvas controller over the pane's xterm container and store
 *  it on the pane. Creation is async (wasm + font load); a pane disposed before
 *  the controller resolves drops it instead of attaching a leaked canvas.
 *
 *  Input/resize route through the (unopened) xterm object's input()/resize() so
 *  they reuse the exact same PTY seam connectPanePty wires for the default path:
 *  input()->onData->transport.sendInput, resize()->onResize->transport.resize.
 *  This keeps intent tracking, presence locks, and replay guards intact.
 *
 *  onFallback runs when async wasm/font init fails on a still-live pane so the
 *  caller can open the normal xterm renderer — turning a would-be black pane
 *  into a working xterm pane.
 *
 *  linkContext (optional) routes terminal URL clicks through orca's opener so the
 *  in-app/system-browser preference is honored; omitted, URLs open in the system
 *  browser. Optional so the existing caller stays backward-compatible. */
export function openAtermPane(
  pane: ManagedPaneInternal,
  onFallback?: () => void,
  linkContext?: AtermLinkContext
): void {
  void createAtermPaneController(
    pane.xtermContainer,
    (data) => pane.terminal.input(data),
    (cols, rows) => pane.terminal.resize(cols, rows),
    // Pastes go through terminal.paste() (not input()) so DECSET 2004 wraps them
    // with \e[200~..\e[201~; input() sends raw and would let an app auto-run paste.
    (text) => pane.terminal.paste(text),
    linkContext,
    // Read macOptionIsMeta live off the (headless) xterm Terminal — the same
    // option applyTerminalAppearance keeps in sync — so the aterm encoder honors
    // a settings toggle without recreating the pane.
    { getMacOptionIsMeta: () => pane.terminal.options.macOptionIsMeta === true }
  )
    .then((controller) => {
      // If the pane was torn down while wasm/font loaded, drop the controller.
      if (pane.disposed) {
        controller.dispose()
        return
      }
      // Guard against double-attach: if a prior controller's mirror is still
      // registered (re-open without a teardown), tear it down before replacing it
      // so we don't leak a duplicate output subscription onto the terminal.
      pane.atermMirrorCleanup?.()
      pane.atermController = controller
      // Mirror PTY output (routed through writeTerminalOutput) onto the canvas.
      pane.atermMirrorCleanup = registerAtermOutputMirror(pane.terminal, controller)
    })
    .catch((err) => {
      console.warn('[aterm] failed to create canvas renderer for pane', pane.id, err)
      // Why: async wasm/font failure would otherwise leave a black pane. Fall
      // back to the normal xterm renderer when the pane is still live so the
      // user gets a working terminal instead. Skip if disposed (teardown).
      if (!pane.disposed) {
        onFallback?.()
      }
    })
}
