import { requireRustGitBinding } from '../daemon/rust-git-addon'
import type { GitHistoryItem } from '../../shared/git-history-types'

// Main-process history-log parsing runs through the verified Rust `orca-git`
// parser via the napi addon — the sole path (the addon is a required main-process
// dependency). Kept out of the shared git-history module so the renderer never
// imports the native binding. The pure TS parser in `git-history-log-parser` still
// backs the addon-less SSH relay and the differential parity oracle; it is NOT a
// runtime fallback here.

/** Parse NUL-delimited `git log` output (GIT_HISTORY_COMMIT_FORMAT) into history
 *  items using the Rust parser. */
export function parseGitHistoryLogNative(stdout: string): GitHistoryItem[] {
  return JSON.parse(requireRustGitBinding().parseGitHistoryLog(stdout)) as GitHistoryItem[]
}
