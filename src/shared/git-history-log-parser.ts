import type { GitHistoryItemRef } from './git-history-types'

export const GIT_HISTORY_COMMIT_FORMAT =
  '%H%n%aN%n%aE%n%at%n%ct%n%P%n%(decorate:prefix=,suffix=,separator=%x1f)%n%B'

export function shortGitHash(hash: string): string {
  return hash.slice(0, 7)
}

export function compareGitHistoryItemRefsByCategory(
  ref1: GitHistoryItemRef,
  ref2: GitHistoryItemRef
): number {
  const order = (ref: GitHistoryItemRef): number => {
    if (ref.id.startsWith('refs/heads/')) {
      return 1
    }
    if (ref.id.startsWith('refs/remotes/')) {
      return 2
    }
    if (ref.id.startsWith('refs/tags/')) {
      return 3
    }
    return 99
  }

  const categoryOrder = order(ref1) - order(ref2)
  return categoryOrder || ref1.name.localeCompare(ref2.name)
}

// parseGitHistoryLog was deleted here: the `git log` stream parser is now the Rust
// `orca_git::git_history_log_parser` core, reached via napi in the main process and
// via wasm in the relay (both inject it into loadGitHistoryFromExecutor). This
// module keeps only the format string + ref helpers the callers still need.

export function gitHistoryRefFromFullName(
  fullName: string | null,
  fallbackName: string,
  revision: string
): GitHistoryItemRef {
  const id = fullName || fallbackName
  if (id.startsWith('refs/heads/')) {
    return { id, name: id.slice('refs/heads/'.length), revision, category: 'branches' }
  }
  if (id.startsWith('refs/remotes/')) {
    return { id, name: id.slice('refs/remotes/'.length), revision, category: 'remote branches' }
  }
  if (id.startsWith('refs/tags/')) {
    return { id, name: id.slice('refs/tags/'.length), revision, category: 'tags' }
  }
  return { id, name: fallbackName || shortGitHash(revision), revision, category: 'commits' }
}
