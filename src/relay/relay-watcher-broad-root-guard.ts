import { homedir } from 'node:os'
import { normalizeRuntimePathForComparison } from '../shared/cross-platform-path'

// Why (#7948): a recursive watch rooted at the account home or a filesystem root
// crawls millions of entries, pinning a relay worker for an hour and freezing
// every workspace sharing the connection; callers tolerate a missing watcher.
export function isBroadWatchRoot(rootPath: string, homePath: string = homedir()): boolean {
  const norm = normalizeRuntimePathForComparison(rootPath)
  const home = normalizeRuntimePathForComparison(homePath)
  if (norm === '' || norm === '/' || /^[a-z]:\/?$/i.test(norm) || norm === home) {
    return true
  }
  // An ancestor of home (e.g. /home or C:/Users) is at least as broad as home itself.
  return home.startsWith(`${norm}/`)
}
