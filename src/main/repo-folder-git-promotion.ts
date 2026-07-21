import { existsSync } from 'node:fs'
import { join } from 'node:path'
import type { Repo } from '../shared/types'
import { isGitRepo } from './git/repo'

type RepoKindStore = {
  getRepos(): Repo[]
  getRepo?(id: string): Repo | undefined
  updateRepo(
    id: string,
    updates: Partial<Pick<Repo, 'kind' | 'externalWorktreeVisibility'>>
  ): Repo | null
}

type PromotionOptions = {
  onChanged?: () => void
}

function getCurrentRepo(store: RepoKindStore, id: string): Repo | undefined {
  return store.getRepo?.(id) ?? store.getRepos().find((repo) => repo.id === id)
}

function promoteFolderRepoIfGitInitialized(store: RepoKindStore, snapshot: Repo): boolean {
  // Why: isGitRepo spawns git synchronously; the cheap .git-entry check keeps
  // list-path scans free until the user actually runs `git init`.
  if (!existsSync(join(snapshot.path, '.git')) || !isGitRepo(snapshot.path)) {
    return false
  }
  const current = getCurrentRepo(store, snapshot.id)
  if (!current || current.kind !== 'folder' || current.path !== snapshot.path) {
    return false
  }
  return !!store.updateRepo(snapshot.id, {
    kind: 'git',
    // Why: git-kind repos added directly default to hiding external worktrees;
    // promotion mirrors that so both paths present the same discovery UX.
    ...(current.externalWorktreeVisibility === undefined
      ? { externalWorktreeVisibility: 'hide' as const }
      : {})
  })
}

/**
 * Promote folder-kind repos to git-kind once a git repository appears at their
 * path (issue #8125: `git init` after add left the Git tab permanently hidden
 * because kind was fixed at add time). Local-path repos only — SSH-connected
 * repos cannot be probed with local filesystem checks and keep their kind.
 */
export function promoteFolderReposWithGitRepositories(
  store: RepoKindStore,
  options: PromotionOptions = {}
): void {
  let changed = false
  for (const repo of store.getRepos()) {
    if (repo.kind !== 'folder' || repo.connectionId) {
      continue
    }
    try {
      changed = promoteFolderRepoIfGitInitialized(store, repo) || changed
    } catch (error) {
      console.warn('[repo-kind] Failed to probe folder repo for git promotion:', error)
    }
  }
  if (changed) {
    options.onChanged?.()
  }
}
