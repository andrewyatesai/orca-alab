import { loadRustGitBinding } from '../daemon/rust-git-addon'
import type { UntrackedAdditionsCounter } from '../../shared/git-uncommitted-line-stats'

/** The Rust orca-git untracked-additions counter (`countAdditionsInBuffer` — the
 *  trailing-newline-aware byte counter, proven by orca-git-napi-parity.test.ts), or
 *  `undefined` when the native addon isn't loadable (an unbuilt dev tree, or the relay,
 *  which ships no per-arch addon). A narrow seam so the count can be wired/tested
 *  independently of the status-parser's use of the same binding. */
export function untrackedAdditionsCounter(): UntrackedAdditionsCounter | undefined {
  return loadRustGitBinding()?.countAdditionsInBuffer
}
