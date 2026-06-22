import type { ManagedPaneInternal } from '../pane-manager-types'
import { createAtermPaneController } from './aterm-pane-renderer'
import { registerAtermOutputMirror } from './aterm-output-mirror'

/** Build the aterm canvas controller over the pane's xterm container and store
 *  it on the pane. Creation is async (wasm + font load); a pane disposed before
 *  the controller resolves drops it instead of attaching a leaked canvas.
 *
 *  Input/resize route through the (unopened) xterm object's input()/resize() so
 *  they reuse the exact same PTY seam connectPanePty wires for the default path:
 *  input()->onData->transport.sendInput, resize()->onResize->transport.resize.
 *  This keeps intent tracking, presence locks, and replay guards intact. */
export function openAtermPane(pane: ManagedPaneInternal): void {
  void createAtermPaneController(
    pane.xtermContainer,
    (data) => pane.terminal.input(data),
    (cols, rows) => pane.terminal.resize(cols, rows)
  )
    .then((controller) => {
      // If the pane was torn down while wasm/font loaded, drop the controller.
      if (pane.disposed) {
        controller.dispose()
        return
      }
      pane.atermController = controller
      // Mirror PTY output (routed through writeTerminalOutput) onto the canvas.
      pane.atermMirrorCleanup = registerAtermOutputMirror(pane.terminal, controller)
    })
    .catch((err) => {
      console.warn('[aterm] failed to create canvas renderer for pane', pane.id, err)
    })
}
