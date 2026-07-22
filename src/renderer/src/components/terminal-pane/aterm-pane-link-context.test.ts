import { describe, expect, it, vi } from 'vitest'
import { buildAtermPaneLinkContext } from './aterm-pane-link-context'

describe('buildAtermPaneLinkContext (#6880 lifecycle binding)', () => {
  it('setUrlLinkContext carries worktreePath/home/runtime getters', () => {
    const getRuntimeEnvironmentIdForPane = vi.fn((paneId: number) =>
      paneId === 7 ? 'ssh-runtime-1' : null
    )
    const context = buildAtermPaneLinkContext(
      {
        worktreeId: 'wt-1',
        worktreePath: '/repo',
        terminalHomePath: '/home/user',
        requestOpenLinksInAppPreference: () => true,
        getPaneLinkCwd: (paneId) => `/cwd-of-${paneId}`,
        getRuntimeEnvironmentIdForPane
      },
      7
    )

    expect(context.worktreeId).toBe('wt-1')
    expect(context.worktreePath).toBe('/repo')
    expect(context.terminalHomePath).toBe('/home/user')
    expect(context.requestOpenLinksInAppPreference?.('https://x.test')).toBe(true)
    expect(context.getStartupCwd?.()).toBe('/cwd-of-7')
    expect(context.getRuntimeEnvironmentId?.()).toBe('ssh-runtime-1')
    expect(getRuntimeEnvironmentIdForPane).toHaveBeenCalledWith(7)
  })

  it('reads cwd and runtime per call so post-bind changes are visible', () => {
    const paneCwds = new Map<number, string>([[3, '/before']])
    let runtimeId: string | null = null
    const context = buildAtermPaneLinkContext(
      {
        worktreeId: 'wt-1',
        worktreePath: '/repo',
        getPaneLinkCwd: (paneId) => paneCwds.get(paneId) ?? '/repo',
        getRuntimeEnvironmentIdForPane: () => runtimeId
      },
      3
    )

    expect(context.getStartupCwd?.()).toBe('/before')
    expect(context.getRuntimeEnvironmentId?.()).toBeNull()

    // Why: OSC 7 changes cwd and runtimes attach after bind — getters must track.
    paneCwds.set(3, '/after')
    runtimeId = 'wsl-runtime-2'
    expect(context.getStartupCwd?.()).toBe('/after')
    expect(context.getRuntimeEnvironmentId?.()).toBe('wsl-runtime-2')
  })

  it('defaults the runtime getter to null when no per-pane resolver exists', () => {
    const context = buildAtermPaneLinkContext(
      {
        worktreeId: 'wt-1',
        worktreePath: '/repo',
        getPaneLinkCwd: () => '/repo'
      },
      1
    )

    expect(context.getRuntimeEnvironmentId?.()).toBeNull()
  })
})
