import { requireRustGitBinding } from '../daemon/rust-git-addon'
import type { GitLineStats } from '../../shared/git-uncommitted-line-stats'

// Main-process numstat parsing runs through the verified Rust `orca-git` parser
// via the napi addon — the sole path (the addon is a required main-process
// dependency). The pure TS parser in `git-uncommitted-line-stats` still backs the
// addon-less SSH relay and the differential parity oracle; it is NOT a runtime
// fallback here.

/** Parse `git diff --numstat` (text or `-z`) into a `path → {added?, removed?}` map
 *  using the Rust parser. The napi parser takes bytes, so the string is re-encoded;
 *  `Buffer.from(s).toString('utf8') === s` for valid input, which is exactly the
 *  parity harness's equivalence. Consumers only `.get(path)` the result, so the Rust
 *  JSON's key order (alphabetical) vs git's output order is irrelevant. */
export function parseNumstatNative(stdout: string): Map<string, GitLineStats> {
  const byPath = JSON.parse(requireRustGitBinding().parseNumstat(Buffer.from(stdout))) as Record<
    string,
    GitLineStats
  >
  return new Map(Object.entries(byPath))
}
