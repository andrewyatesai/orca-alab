import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { Repo } from '../shared/types'

const { existsSyncMock, isGitRepoMock } = vi.hoisted(() => ({
  existsSyncMock: vi.fn(),
  isGitRepoMock: vi.fn()
}))

vi.mock('node:fs', async (importOriginal) => {
  const actual = await importOriginal<typeof import('node:fs')>() // eslint-disable-line @typescript-eslint/consistent-type-imports -- importOriginal requires inline import()
  return { ...actual, existsSync: existsSyncMock }
})

vi.mock('./git/repo', () => ({
  isGitRepo: isGitRepoMock
}))

import { promoteFolderReposWithGitRepositories } from './repo-folder-git-promotion'

function makeRepo(overrides: Partial<Repo>): Repo {
  return {
    id: 'repo-1',
    path: '/projects/notes',
    displayName: 'notes',
    badgeColor: '#000000',
    addedAt: 0,
    kind: 'folder',
    ...overrides
  } as Repo
}

function makeStore(repos: Repo[]) {
  const byId = new Map(repos.map((repo) => [repo.id, repo]))
  return {
    getRepos: () => [...byId.values()],
    getRepo: (id: string) => byId.get(id),
    updateRepo: vi.fn((id: string, updates: Partial<Repo>) => {
      const current = byId.get(id)
      if (!current) {
        return null
      }
      const updated = { ...current, ...updates }
      byId.set(id, updated)
      return updated
    })
  }
}

describe('promoteFolderReposWithGitRepositories', () => {
  beforeEach(() => {
    existsSyncMock.mockReset().mockReturnValue(false)
    isGitRepoMock.mockReset().mockReturnValue(false)
  })

  it('promotes a folder repo to git-kind after git init appears at its path', () => {
    existsSyncMock.mockReturnValue(true)
    isGitRepoMock.mockReturnValue(true)
    const store = makeStore([makeRepo({})])
    const onChanged = vi.fn()

    promoteFolderReposWithGitRepositories(store, { onChanged })

    expect(store.updateRepo).toHaveBeenCalledWith('repo-1', {
      kind: 'git',
      externalWorktreeVisibility: 'hide'
    })
    expect(onChanged).toHaveBeenCalledOnce()
  })

  it('skips the git spawn entirely while no .git entry exists', () => {
    const store = makeStore([makeRepo({})])
    const onChanged = vi.fn()

    promoteFolderReposWithGitRepositories(store, { onChanged })

    expect(isGitRepoMock).not.toHaveBeenCalled()
    expect(store.updateRepo).not.toHaveBeenCalled()
    expect(onChanged).not.toHaveBeenCalled()
  })

  it('leaves a .git entry that is not a real repository unpromoted', () => {
    existsSyncMock.mockReturnValue(true)
    isGitRepoMock.mockReturnValue(false)
    const store = makeStore([makeRepo({})])
    const onChanged = vi.fn()

    promoteFolderReposWithGitRepositories(store, { onChanged })

    expect(store.updateRepo).not.toHaveBeenCalled()
    expect(onChanged).not.toHaveBeenCalled()
  })

  it('never probes git-kind or SSH-connected repos', () => {
    existsSyncMock.mockReturnValue(true)
    isGitRepoMock.mockReturnValue(true)
    const store = makeStore([
      makeRepo({ id: 'git-repo', kind: 'git' }),
      makeRepo({ id: 'ssh-folder', connectionId: 'conn-1' })
    ])

    promoteFolderReposWithGitRepositories(store)

    expect(existsSyncMock).not.toHaveBeenCalled()
    expect(store.updateRepo).not.toHaveBeenCalled()
  })

  it('preserves an existing externalWorktreeVisibility value on promotion', () => {
    existsSyncMock.mockReturnValue(true)
    isGitRepoMock.mockReturnValue(true)
    const store = makeStore([makeRepo({ externalWorktreeVisibility: 'show' })])

    promoteFolderReposWithGitRepositories(store)

    expect(store.updateRepo).toHaveBeenCalledWith('repo-1', { kind: 'git' })
  })

  it('keeps scanning remaining repos when one probe throws', () => {
    existsSyncMock.mockImplementation((probePath: string) => {
      if (String(probePath).includes('broken')) {
        throw new Error('EACCES')
      }
      return true
    })
    isGitRepoMock.mockReturnValue(true)
    const store = makeStore([
      makeRepo({ id: 'broken', path: '/projects/broken' }),
      makeRepo({ id: 'ok', path: '/projects/ok' })
    ])
    const onChanged = vi.fn()

    promoteFolderReposWithGitRepositories(store, { onChanged })

    expect(store.updateRepo).toHaveBeenCalledWith('ok', expect.objectContaining({ kind: 'git' }))
    expect(onChanged).toHaveBeenCalledOnce()
  })
})
