// Per-terminal monotonic write generation, bumped by every non-empty write that
// reaches a terminal through the two output funnels (scheduler + foreground/replay
// chunk writes). Deep scrollback hydration captures it after the mount-time tail
// replay and aborts its rebuild if ANY other writer (live PTY bytes, reattach or
// cold-restore structural replay) touched the terminal in between — a rebuild
// over foreign content would destroy it.

const writeGenerationsByTerminal = new WeakMap<object, number>()

export function bumpTerminalWriteGeneration(terminal: object): void {
  writeGenerationsByTerminal.set(terminal, (writeGenerationsByTerminal.get(terminal) ?? 0) + 1)
}

export function getTerminalWriteGeneration(terminal: object): number {
  return writeGenerationsByTerminal.get(terminal) ?? 0
}
