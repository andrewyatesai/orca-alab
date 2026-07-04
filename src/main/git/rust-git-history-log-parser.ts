import { loadRustGitBinding } from '../daemon/rust-git-addon'
import { parseGitHistoryLog } from '../../shared/git-history-log-parser'
import type { GitHistoryItem } from '../../shared/git-history-types'

// Main-process-only wrapper: prefer the verified Rust `orca-git` history-log parser
// (via the napi addon) with the pure TS parser as the proven-identical fallback.
// Kept out of the shared git-history module so the renderer never imports the native
// binding — the same cutover shape as git-status-stream and parseWorktreeList. The
// two are held in lockstep by the orca-parity `parseGitHistoryLog` differential
// vectors and orca-git-napi-parity.test.

/** Parse NUL-delimited `git log` output (GIT_HISTORY_COMMIT_FORMAT) into history
 *  items, using the Rust parser when the addon loads and the TS parser otherwise. */
export function parseGitHistoryLogPreferRust(stdout: string): GitHistoryItem[] {
  const binding = loadRustGitBinding()
  if (binding) {
    try {
      return JSON.parse(binding.parseGitHistoryLog(stdout)) as GitHistoryItem[]
    } catch {
      // A bad/incompatible addon must never break the history panel — fall through
      // to the identical TS parser.
    }
  }
  return parseGitHistoryLog(stdout)
}
