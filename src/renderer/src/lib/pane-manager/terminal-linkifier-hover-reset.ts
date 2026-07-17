import type { AtermTerminalFacade } from './aterm/aterm-terminal-facade'

/**
 * Force the pane's link hit-testing to re-run on the next mousemove.
 *
 * Why: the aterm link input caches the last hovered cell and short-circuits
 * mousemoves that land on the same cell. Panes survive worktree/tab switches
 * (facades stay alive while hidden), so when the pointer returns to the same
 * cell on reveal the cached hover can be stale — buffer content may have
 * changed while hidden — and the link stays dead or outdated until the pointer
 * crosses a cell boundary. Clearing the cell cache makes the next mousemove
 * re-evaluate the engine link_at + registered providers (upstream #9061,
 * re-derived from xterm's linkifier `_lastBufferCell` reset onto the facade).
 */
export function resetTerminalLinkifierHoverState(
  terminal: Partial<Pick<AtermTerminalFacade, 'resetLinkHoverCache'>>
): void {
  // Best-effort, mirroring upstream's guarded reset: resume also runs against
  // partial terminal doubles that never wire the pointer-input stack.
  terminal.resetLinkHoverCache?.()
}
