import type { AtermTerminalFacade } from './aterm/aterm-terminal-facade'

// Per-terminal monotonic output counter: scroll-restore heuristics compare
// epochs to tell "new output arrived since capture" apart from a quiet pane.
const terminalOutputEpochs = new WeakMap<AtermTerminalFacade, number>()

export function recordTerminalOutput(terminal: AtermTerminalFacade): void {
  terminalOutputEpochs.set(terminal, getTerminalOutputEpoch(terminal) + 1)
}

export function getTerminalOutputEpoch(terminal: AtermTerminalFacade): number {
  return terminalOutputEpochs.get(terminal) ?? 0
}
