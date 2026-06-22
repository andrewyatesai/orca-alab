import type { AtermPaneController } from './aterm-pane-renderer'

// Why a terminal-keyed registry: the output scheduler is keyed by the xterm
// terminal object and has no pane reference. Registering the pane's aterm
// controller against its terminal lets writeTerminalOutput mirror PTY bytes to
// the canvas without the scheduler taking a dependency on pane-manager types.
// WeakMap so a disposed terminal drops its controller binding automatically.
const controllersByTerminal = new WeakMap<object, AtermPaneController>()

export function registerAtermOutputMirror(
  terminal: object,
  controller: AtermPaneController
): () => void {
  controllersByTerminal.set(terminal, controller)
  return () => {
    if (controllersByTerminal.get(terminal) === controller) {
      controllersByTerminal.delete(terminal)
    }
  }
}

export function mirrorOutputToAterm(terminal: object, data: string): void {
  controllersByTerminal.get(terminal)?.process(data)
}
