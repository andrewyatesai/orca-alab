import { loadRustGitBinding } from '../daemon/rust-git-addon'
import {
  parseNumstat as parseNumstatTs,
  type GitLineStats
} from '../../shared/git-uncommitted-line-stats'

// Main-process-only wrapper: prefer the verified Rust `orca-git` numstat parser
// (via the napi addon) with the pure TS parser as the proven-identical fallback.
// The napi export already ships and is exercised by orca-git-napi-parity.test;
// this routes production through it. Consumers only `.get(path)` the result, so the
// Rust JSON's key order (alphabetical) vs git's output order is irrelevant.

/** Parse `git diff --numstat` (text or `-z`) into a `path → {added?, removed?}` map,
 *  using the Rust parser when the addon loads and the TS parser otherwise. The napi
 *  parser takes bytes, so the string is re-encoded; `Buffer.from(s).toString('utf8')
 *  === s` for valid input, which is exactly the parity harness's equivalence. */
export function parseNumstatPreferRust(stdout: string): Map<string, GitLineStats> {
  const binding = loadRustGitBinding()
  if (binding) {
    try {
      const byPath = JSON.parse(binding.parseNumstat(Buffer.from(stdout))) as Record<
        string,
        GitLineStats
      >
      return new Map(Object.entries(byPath))
    } catch {
      // A bad/incompatible addon must never break status parsing — fall through
      // to the identical TS parser.
    }
  }
  return parseNumstatTs(stdout)
}
