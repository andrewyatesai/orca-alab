// Main-process terminal quick-command sanitizer, driven by the Rust orca-agents
// core via napi (the shared TS body was deleted). The main process only needs
// the untrusted-input normalizer (persistence sanitizes on load + on update);
// the renderer drives the fuller helper set through the same op via wasm.
import { requireRustGitBinding } from './daemon/rust-git-addon'
import type { TerminalQuickCommand } from '../shared/types'

export function normalizeTerminalQuickCommands(input: unknown): TerminalQuickCommand[] {
  return JSON.parse(
    requireRustGitBinding().terminalQuickCommandOp(
      'normalizeTerminalQuickCommands',
      JSON.stringify(input ?? null)
    )
  ) as TerminalQuickCommand[]
}
