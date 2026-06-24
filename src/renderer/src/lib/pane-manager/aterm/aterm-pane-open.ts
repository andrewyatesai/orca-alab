import type { ManagedPaneInternal } from '../pane-manager-types'
import { useAppStore } from '@/store'
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
    // Route input through connectPanePty's PTY pipeline directly (intent/presence/
    // replay), bypassing the xterm shim, once it's wired; fall back to xterm.input
    // only in the pre-connect window. Drained query replies use the same sink.
    (data) => (pane.routePtyInput ? pane.routePtyInput(data) : pane.terminal.input(data)),
    // Resize straight to the PTY pipeline (presence/held-resize gates), bypassing
    // the shim — its buffer dims are unused for aterm panes. Pre-connect fallback only.
    (cols, rows) =>
      pane.routePtyResize ? pane.routePtyResize(cols, rows) : pane.terminal.resize(cols, rows),
    // Paste, wrapped + routed natively (off the xterm shim). Matches xterm.paste:
    // normalize \r?\n→\r, and in bracketed-paste mode (DECSET 2004) wrap in
    // ESC[200~..ESC[201~ with embedded ESC bytes neutralized (paste-injection guard)
    // so an app gets one atomic, un-auto-run paste. Routes through the PTY pipeline
    // (pane.routePtyInput); pre-connect it falls back to the shim.
    (text) => {
      const normalized = text.replace(/\r?\n/g, '\r')
      const data = pane.atermController?.bracketedPasteMode()
        ? `\x1b[200~${normalized.replace(/\x1b/g, '␛')}\x1b[201~`
        : normalized
      if (pane.routePtyInput) {
        pane.routePtyInput(data)
      } else {
        pane.terminal.paste(text)
      }
    },
    linkContext,
    // Read macOptionIsMeta live off the (headless) xterm Terminal — the same
    // option applyTerminalAppearance keeps in sync — so the aterm encoder honors
    // a settings toggle without recreating the pane. copy-on-select is read live
    // off the app settings (default false) so drag-select doesn't clobber the
    // clipboard unless the user opted in.
    {
      getMacOptionIsMeta: () => pane.terminal.options.macOptionIsMeta === true,
      getCopyOnSelect: () => useAppStore.getState().settings?.terminalClipboardOnSelect === true,
      // Mirror xterm's cursorBlink (kept in sync on the headless Terminal by
      // applyTerminalAppearance) so the aterm cursor honors the toggle live.
      getCursorBlink: () => pane.terminal.options.cursorBlink !== false
    }
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
