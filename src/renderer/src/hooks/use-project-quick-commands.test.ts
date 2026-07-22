// Init the orca-git wasm so resolveHookCommandSourcePolicy returns real Rust
// results instead of the pre-ready 'shared-only' fallback (node-env test).
import '@/lib/git-wasm/init-git-wasm-for-test'
import { describe, expect, it, vi } from 'vitest'
import type { Repo } from '../../../shared/types'
import { getSharedCommandTrustContent } from '../../../shared/orca-yaml-trust-content'
import { hashOrcaHookScript } from '@/lib/orca-hook-trust'

const ensureHooksConfirmedMock = vi.hoisted(() => vi.fn())

vi.mock('@/store', () => ({
  useAppStore: Object.assign(vi.fn(), {
    getState: () => ({ repos: [], trustedOrcaHooks: {} })
  })
}))
vi.mock('@/store/selectors', () => ({ useRepoById: vi.fn(() => null) }))
vi.mock('@/runtime/runtime-hooks-client', () => ({ checkRuntimeHooks: vi.fn() }))
vi.mock('@/lib/ensure-hooks-confirmed', () => ({
  ensureHooksConfirmed: ensureHooksConfirmedMock,
  settingsForHookRepoOwner: vi.fn()
}))

import {
  isProjectQuickCommandSourceLocalOnly,
  resolveProjectQuickCommandsSnapshot,
  reviewProjectQuickCommandTrust
} from './use-project-quick-commands'

function makeRepo(commandSourcePolicy?: string): Pick<Repo, 'hookSettings'> {
  return {
    hookSettings: {
      mode: 'auto',
      commandSourcePolicy,
      scripts: { setup: 'echo local', archive: '' }
    }
  } as unknown as Pick<Repo, 'hookSettings'>
}

describe('resolveProjectQuickCommandsSnapshot', () => {
  const emptySnapshot = { commands: [], sharedTrustContentHash: null }

  // Why: fail closed — yaml Orca could not inspect must never surface runnable entries.
  it('resolves to no commands when the hooks inspection errored', async () => {
    await expect(
      resolveProjectQuickCommandsSnapshot('repo-1', {
        status: 'error',
        hasHooks: false,
        hooks: null,
        mayNeedUpdate: false
      })
    ).resolves.toEqual(emptySnapshot)
  })

  it('resolves to no commands when orca.yaml has no quickCommands', async () => {
    await expect(
      resolveProjectQuickCommandsSnapshot('repo-1', {
        status: 'ok',
        hasHooks: true,
        hooks: { scripts: { setup: 'pnpm install' } },
        mayNeedUpdate: false
      })
    ).resolves.toEqual(emptySnapshot)
  })

  it('hashes the same snapshot the commands came from', async () => {
    const hooks = {
      scripts: { setup: 'pnpm install' },
      quickCommands: [{ label: 'Dev server', command: 'npm run dev' }]
    }
    const snapshot = await resolveProjectQuickCommandsSnapshot('repo-1', {
      status: 'ok',
      hasHooks: true,
      hooks,
      mayNeedUpdate: false
    })
    expect(snapshot.commands.map((command) => command.id)).toEqual(['orcaYaml:repo-1:0'])
    expect(snapshot.sharedTrustContentHash).toBe(
      await hashOrcaHookScript(getSharedCommandTrustContent(hooks))
    )
  })
})

describe('isProjectQuickCommandSourceLocalOnly', () => {
  it('local-only command source policy suppresses orca.yaml quick commands', () => {
    expect(isProjectQuickCommandSourceLocalOnly(makeRepo('local-only'))).toBe(true)
    expect(isProjectQuickCommandSourceLocalOnly(makeRepo('shared-only'))).toBe(false)
    expect(isProjectQuickCommandSourceLocalOnly(makeRepo('run-both'))).toBe(false)
  })
})

describe('reviewProjectQuickCommandTrust', () => {
  it("routes through the shared 'setup' trust review and returns the decision", async () => {
    ensureHooksConfirmedMock.mockResolvedValue('skip')
    await expect(reviewProjectQuickCommandTrust('repo-1')).resolves.toBe('skip')
    expect(ensureHooksConfirmedMock).toHaveBeenCalledWith(expect.anything(), 'repo-1', 'setup')
  })
})
