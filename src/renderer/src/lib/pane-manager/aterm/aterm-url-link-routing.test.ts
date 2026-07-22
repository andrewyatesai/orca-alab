/**
 * @vitest-environment happy-dom
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createAtermOscLinkOpener, type AtermLinkContext } from './aterm-url-link-routing'
import { openDetectedFilePath } from '@/components/terminal-pane/terminal-file-open-routing'
import { openTerminalHttpLink } from '@/components/terminal-pane/terminal-url-link-hit-testing'

// Why: these tests pin the scheme ROUTING decisions (#6880); the open side
// effects (stat/editor/browser) have their own suites.
vi.mock('@/components/terminal-pane/terminal-file-open-routing', () => ({
  openDetectedFilePath: vi.fn()
}))
vi.mock('@/components/terminal-pane/terminal-url-link-hit-testing', () => ({
  openTerminalHttpLink: vi.fn()
}))

function clickWithModifier(): MouseEvent {
  return new MouseEvent('click', { ctrlKey: true })
}

beforeEach(() => {
  vi.clearAllMocks()
  // Why: pin a non-Mac UA so ctrl+click activates regardless of the host OS.
  vi.stubGlobal('navigator', { userAgent: 'X11; Linux x86_64' })
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('createAtermOscLinkOpener (#6880 kind-0 scheme routing)', () => {
  function makeContext(overrides: Partial<AtermLinkContext> = {}): AtermLinkContext {
    return {
      worktreeId: 'wt-1',
      worktreePath: '/repo',
      terminalHomePath: '/home/user',
      getStartupCwd: () => '/repo/src',
      getRuntimeEnvironmentId: () => null,
      ...overrides
    }
  }

  it('file:// OSC 8 target opens via openDetectedFilePath with line/column', () => {
    const opener = createAtermOscLinkOpener(() => makeContext())

    opener('file:///repo/src/main.ts#L42C7', clickWithModifier())

    expect(openDetectedFilePath).toHaveBeenCalledTimes(1)
    expect(openDetectedFilePath).toHaveBeenCalledWith(
      '/repo/src/main.ts',
      42,
      7,
      expect.objectContaining({
        worktreeId: 'wt-1',
        worktreePath: '/repo',
        runtimeEnvironmentId: null,
        openWithSystemDefault: false
      })
    )
    expect(openTerminalHttpLink).not.toHaveBeenCalled()
  })

  it('C:\\ path target routes as a path before URL parsing (Windows)', () => {
    const opener = createAtermOscLinkOpener(() =>
      makeContext({
        worktreePath: 'C:\\repo',
        terminalHomePath: 'C:\\Users\\user',
        getStartupCwd: () => 'C:\\repo'
      })
    )

    // Why: new URL("C:\\...") succeeds with protocol "c:" — the path branch must win.
    opener('C:\\repo\\src\\main.ts', clickWithModifier())

    expect(openDetectedFilePath).toHaveBeenCalledTimes(1)
    expect(vi.mocked(openDetectedFilePath).mock.calls[0][0]).toContain('main.ts')
    expect(openTerminalHttpLink).not.toHaveBeenCalled()
  })

  it('mailto:/unknown scheme is a no-op', () => {
    const opener = createAtermOscLinkOpener(() => makeContext())

    opener('mailto:dev@example.com', clickWithModifier())
    opener('vscode://file/repo/src/main.ts', clickWithModifier())

    expect(openDetectedFilePath).not.toHaveBeenCalled()
    expect(openTerminalHttpLink).not.toHaveBeenCalled()
  })

  it('startupCwd getter is read per click, not captured at bind', () => {
    let cwd = '/first'
    const opener = createAtermOscLinkOpener(() => makeContext({ getStartupCwd: () => cwd }))

    opener('src/main.ts', clickWithModifier())
    cwd = '/second'
    opener('src/main.ts', clickWithModifier())

    expect(openDetectedFilePath).toHaveBeenCalledTimes(2)
    expect(vi.mocked(openDetectedFilePath).mock.calls[0][0]).toBe('/first/src/main.ts')
    expect(vi.mocked(openDetectedFilePath).mock.calls[1][0]).toBe('/second/src/main.ts')
  })

  it('http(s) targets keep the in-app/system-browser preference plumbing', () => {
    const requestOpenLinksInAppPreference = vi.fn(() => true)
    const opener = createAtermOscLinkOpener(() =>
      makeContext({ requestOpenLinksInAppPreference })
    )

    opener('https://example.test/path', clickWithModifier())

    expect(openTerminalHttpLink).toHaveBeenCalledTimes(1)
    expect(openTerminalHttpLink).toHaveBeenCalledWith(
      'https://example.test/path',
      expect.objectContaining({
        worktreeId: 'wt-1',
        forceSystemBrowser: false,
        requestOpenLinksInAppPreference
      })
    )
    expect(openDetectedFilePath).not.toHaveBeenCalled()
  })

  it('does not open anything without the platform activation modifier', () => {
    const opener = createAtermOscLinkOpener(() => makeContext())

    opener('file:///repo/src/main.ts', new MouseEvent('click'))

    expect(openDetectedFilePath).not.toHaveBeenCalled()
    expect(openTerminalHttpLink).not.toHaveBeenCalled()
  })
})
