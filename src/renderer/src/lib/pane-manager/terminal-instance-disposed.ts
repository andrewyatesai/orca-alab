import type { AtermTerminalFacade } from './aterm/aterm-terminal-facade-types'

// Why a disposed probe: a restore write into an already-disposed terminal is
// wasted — the aterm facade's feedEngine() no-ops after dispose (it still fires
// the write completion callback, unlike xterm which silently drops it). Naming
// that moment in a breadcrumb and skipping the replay avoids a futile
// replay-guard cycle against a dead pane. The facade exposes a real `isDisposed`
// flag (set in dispose()); read it structurally so a non-facade value degrades
// to false (treated as live) rather than throwing.
export function isTerminalInstanceDisposed(terminal: unknown): boolean {
  return (terminal as Partial<AtermTerminalFacade> | null)?.isDisposed === true
}
