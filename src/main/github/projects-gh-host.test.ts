import { beforeEach, describe, expect, it } from 'vitest'
import {
  _resetProjectsGhHostForTests,
  projectsGhHostArgs,
  projectsGhHostForProject,
  registerProjectsHostRemoteInventory,
  rememberProjectsGhHostForOwner,
  rememberProjectsGhHostForProject,
  resolveProjectsGhHost
} from './projects-gh-host'

describe('projects gh host derivation (#1715)', () => {
  beforeEach(() => {
    _resetProjectsGhHostForTests()
  })

  it('returns null with no registered inventory (gh default host)', () => {
    expect(resolveProjectsGhHost('acme')).toBeNull()
  })

  it('derives a GHES host from a matching repo remote', () => {
    registerProjectsHostRemoteInventory(() => [
      { remoteUrl: 'git@ghe.corp.example:acme/app.git' },
      { remoteUrl: 'https://github.com/other/lib.git' }
    ])
    expect(resolveProjectsGhHost('acme')).toBe('ghe.corp.example')
  })

  it('matches owners case-insensitively', () => {
    registerProjectsHostRemoteInventory(() => [
      { remoteUrl: 'https://ghe.corp.example/Acme/app.git' }
    ])
    expect(resolveProjectsGhHost('ACME')).toBe('ghe.corp.example')
  })

  it('returns null for owners whose repos live on github.com', () => {
    registerProjectsHostRemoteInventory(() => [{ remoteUrl: 'git@github.com:acme/app.git' }])
    expect(resolveProjectsGhHost('acme')).toBeNull()
  })

  it('returns null when an owner is ambiguous across hosts', () => {
    // Why: an owner seen on both github.com and GHES cannot be pinned safely.
    registerProjectsHostRemoteInventory(() => [
      { remoteUrl: 'git@github.com:acme/app.git' },
      { remoteUrl: 'git@ghe.corp.example:acme/tools.git' }
    ])
    expect(resolveProjectsGhHost('acme')).toBeNull()
  })

  it('returns null for unknown owners and non-GitHub-shaped remotes', () => {
    registerProjectsHostRemoteInventory(() => [
      { remoteUrl: 'git@ghe.corp.example:acme/app.git' },
      { remoteUrl: 'not a url' }
    ])
    expect(resolveProjectsGhHost('someone-else')).toBeNull()
  })

  it('honors hosts learned outside the inventory', () => {
    rememberProjectsGhHostForOwner('acme', 'ghe.corp.example')
    expect(resolveProjectsGhHost('acme')).toBe('ghe.corp.example')
  })

  it('never learns github.com as a pinned owner host', () => {
    rememberProjectsGhHostForOwner('acme', 'github.com')
    expect(resolveProjectsGhHost('acme')).toBeNull()
  })

  it('stamps and returns per-project hosts for node-id mutations', () => {
    rememberProjectsGhHostForProject('PVT_node1', 'ghe.corp.example')
    expect(projectsGhHostForProject('PVT_node1')).toBe('ghe.corp.example')
    expect(projectsGhHostForProject('PVT_other')).toBeNull()
  })

  it('ignores github.com and null project stamps', () => {
    rememberProjectsGhHostForProject('PVT_node1', 'github.com')
    rememberProjectsGhHostForProject('PVT_node2', null)
    expect(projectsGhHostForProject('PVT_node1')).toBeNull()
    expect(projectsGhHostForProject('PVT_node2')).toBeNull()
  })

  it('builds --hostname args only for non-default hosts', () => {
    expect(projectsGhHostArgs('ghe.corp.example')).toEqual(['--hostname', 'ghe.corp.example'])
    expect(projectsGhHostArgs('github.com')).toEqual([])
    expect(projectsGhHostArgs(null)).toEqual([])
    expect(projectsGhHostArgs(undefined)).toEqual([])
  })
})
