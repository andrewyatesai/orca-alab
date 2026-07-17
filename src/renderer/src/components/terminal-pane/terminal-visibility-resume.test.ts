// Fork adaptation of upstream's terminal-visibility-resume.test.ts: the
// xterm-era reveal-repaint assertions were superseded by the fork's aterm
// resume architecture (write-freeze + re-anchor; covered by the scroll-intent
// and pane-lifecycle suites). This keeps the upstream #9061 coverage: reveal
// must reset every pane's link hover cache so links recover without a scroll.
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { PaneManager } from '@/lib/pane-manager/pane-manager'
import { resumeTerminalVisibility } from './terminal-visibility-resume'

vi.mock('@/lib/pane-manager/pane-manager-registry', () => ({
  resetAllTerminalWebglAtlases: vi.fn(),
  resetAndRefreshAllTerminalWebglAtlases: vi.fn()
}))
vi.mock('@/lib/pane-manager/pane-terminal-output-scheduler', () => ({
  flushTerminalOutput: vi.fn(),
  requestTerminalBacklogRecovery: vi.fn()
}))
vi.mock('@/lib/pane-manager/terminal-scroll-intent', () => ({
  beginSuppressScrollIntentWrites: vi.fn(),
  endSuppressScrollIntentWrites: vi.fn(),
  enforceTerminalCurrentScrollIntent: vi.fn(),
  syncTerminalScrollIntentFromViewport: vi.fn()
}))
vi.mock('./pane-helpers', () => ({
  fitAndFocusPanes: vi.fn(),
  fitPanes: vi.fn(),
  focusActivePane: vi.fn()
}))
vi.mock('./terminal-webgl-atlas-recovery', () => ({
  scheduleTabRevealWebglAtlasRecovery: vi.fn()
}))
const resetTerminalLinkifierHoverState = vi.fn()
vi.mock('@/lib/pane-manager/terminal-linkifier-hover-reset', () => ({
  resetTerminalLinkifierHoverState: (terminal: unknown) =>
    resetTerminalLinkifierHoverState(terminal)
}))

function createManager(panes: { terminal: unknown }[]): PaneManager {
  return {
    getPanes: vi.fn(() => panes),
    resumeRendering: vi.fn(),
    suspendRendering: vi.fn(),
    scheduleRevealRepaint: vi.fn(),
    scheduleRevealPresent: vi.fn()
  } as unknown as PaneManager
}

function resumeArgs(manager: PaneManager, shouldUseLightTabResume: boolean) {
  return {
    manager,
    isActive: true,
    wasVisible: false,
    shouldUseLightTabResume,
    captureViewportPositions: vi.fn(() => new Map())
  }
}

describe('resumeTerminalVisibility link hover recovery', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it.each([true, false])(
    'resets each pane linkifier hover cache on reveal (light=%s) so links recover without a scroll',
    (light) => {
      const first = { name: 'pane-a' }
      const second = { name: 'pane-b' }
      const manager = createManager([{ terminal: first }, { terminal: second }])

      resumeTerminalVisibility(resumeArgs(manager, light))

      expect(resetTerminalLinkifierHoverState).toHaveBeenCalledWith(first)
      expect(resetTerminalLinkifierHoverState).toHaveBeenCalledWith(second)
    }
  )
})
