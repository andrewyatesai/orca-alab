import { describe, expect, it } from 'vitest'
import { getDevInstanceIdentity } from './dev-instance-identity'

describe('dev-instance-identity', () => {
  it('keeps packaged identity stable', () => {
    expect(getDevInstanceIdentity(false, {})).toMatchObject({
      name: 'Orca',
      appName: 'Orca',
      isDev: false,
      devLabel: null,
      dockBadgeLabel: null,
      appUserModelId: 'com.stablyai.orca'
    })
  })

  // Why: fork staging builds inject productName via electron-builder
  // extraMetadata (audit F14); the packaged identity must echo it back so
  // app.setName never flips userData onto public Orca's directory, and the
  // AUMID must match the staging appId for Windows taskbar/notifications.
  it('adopts the injected fork productName and staging AUMID when packaged', () => {
    expect(getDevInstanceIdentity(false, {}, 'Orca Staging')).toMatchObject({
      name: 'Orca Staging',
      appName: 'Orca Staging',
      isDev: false,
      appUserModelId: 'com.stablyai.orca.staging'
    })
  })

  it('keeps the upstream packaged identity for public-identity builds', () => {
    // 'orca' is the raw package.json name — what Electron reports when no
    // productName was injected (ORCA_PUBLIC_IDENTITY=1 diff builds).
    expect(getDevInstanceIdentity(false, {}, 'orca')).toMatchObject({
      name: 'Orca',
      appName: 'Orca',
      appUserModelId: 'com.stablyai.orca'
    })
    expect(getDevInstanceIdentity(false, {}, 'Orca')).toMatchObject({
      name: 'Orca',
      appName: 'Orca',
      appUserModelId: 'com.stablyai.orca'
    })
  })

  it('pins a stable dev appName across branches so the safeStorage key does not churn', () => {
    const a = getDevInstanceIdentity(true, { ORCA_DEV_BRANCH: 'feature/a' })
    const b = getDevInstanceIdentity(true, { ORCA_DEV_BRANCH: 'feature/b' })

    // Per-branch identity differs via devLabel (name is the pinned edition dock title)...
    expect(a.devLabel).not.toBe(b.devLabel)
    expect(a.name).toBe('Orca: ALab Edition')
    // ...but the Keychain-driving appName is identical and distinct from prod.
    expect(a.appName).toBe('Orca Dev')
    expect(b.appName).toBe('Orca Dev')
    expect(a.appName).not.toBe('Orca')
  })

  it('derives a readable dev label from worktree and branch env', () => {
    const identity = getDevInstanceIdentity(true, {
      ORCA_DEV_REPO_ROOT: '/repo/worktrees/dev-indicator',
      ORCA_DEV_WORKTREE_NAME: 'dev-indicator',
      ORCA_DEV_BRANCH: 'nwparker/dev-indicator'
    })

    expect(identity).toMatchObject({
      isDev: true,
      devLabel: 'dev-indicator',
      devBranch: 'nwparker/dev-indicator',
      devWorktreeName: 'dev-indicator',
      devRepoRoot: '/repo/worktrees/dev-indicator'
    })
    expect(identity.name).toBe('Orca: ALab Edition')
    expect(identity.dockBadgeLabel).toBeNull()
    expect(identity.appUserModelId).toMatch(/^com\.stablyai\.orca\.dev\.[a-f0-9]{10}$/)
  })

  it('includes the branch when it differs from the worktree basename', () => {
    const identity = getDevInstanceIdentity(true, {
      ORCA_DEV_REPO_ROOT: '/repo/worktrees/payment-ui',
      ORCA_DEV_WORKTREE_NAME: 'payment-ui',
      ORCA_DEV_BRANCH: 'feature/billing-shell'
    })

    expect(identity.devLabel).toBe('payment-ui @ feature/billing-shell')
    expect(identity.name).toBe('Orca: ALab Edition')
    expect(identity.dockBadgeLabel).toBeNull()
  })

  it('allows an explicit label override', () => {
    const identity = getDevInstanceIdentity(true, {
      ORCA_DEV_INSTANCE_LABEL: 'manual label',
      ORCA_DEV_WORKTREE_NAME: 'dev-indicator',
      ORCA_DEV_BRANCH: 'feature/other'
    })

    expect(identity.devLabel).toBe('manual label')
    expect(identity.name).toBe('Orca: ALab Edition')
    expect(identity.dockBadgeLabel).toBeNull()
  })
})
