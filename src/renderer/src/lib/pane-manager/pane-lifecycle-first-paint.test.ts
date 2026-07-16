/**
 * @vitest-environment happy-dom
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { TerminalLeafId } from '../../../../shared/stable-pane-id'
import type { PaneManagerOptions } from './pane-manager-types'
import type { DragReorderCallbacks, DragReorderState } from './pane-drag-reorder'

// The engine/facade layers are exercised elsewhere; this test targets the
// never-blank first paint seam only.
vi.mock('./aterm/aterm-pane-open', () => ({ openAtermPane: vi.fn() }))
vi.mock('./aterm/aterm-theme-colors', () => ({
  resolveAtermThemeColors: vi.fn(() => ({ bg: 0x0a0a0a }))
}))
vi.mock('./aterm/aterm-terminal-facade', () => ({
  createAtermTerminalFacade: vi.fn(() => ({ options: {} }))
}))
vi.mock('./aterm/aterm-addon-facades', () => ({
  createAtermFitAddonFacade: vi.fn(() => ({})),
  createAtermSearchAddonFacade: vi.fn(() => ({})),
  createAtermSerializeAddonFacade: vi.fn(() => ({}))
}))

import { createPaneDOM } from './pane-lifecycle'
import { resolveAtermThemeColors } from './aterm/aterm-theme-colors'

const LEAF_ID = '11111111-1111-4111-8111-111111111111' as TerminalLeafId

function buildPane(): ReturnType<typeof createPaneDOM> {
  return createPaneDOM(
    1,
    LEAF_ID,
    {} as PaneManagerOptions,
    {} as DragReorderState,
    {} as DragReorderCallbacks,
    vi.fn(),
    vi.fn()
  )
}

describe('createPaneDOM never-blank first paint', () => {
  beforeEach(() => {
    vi.mocked(resolveAtermThemeColors).mockReturnValue({ bg: 0x0a0a0a } as never)
  })

  it('paints the pane container with the active theme background at creation', () => {
    const pane = buildPane()
    expect(pane.container.style.background).toBe('#0a0a0a')
  })

  it('follows the resolved theme, not a hardcoded color', () => {
    vi.mocked(resolveAtermThemeColors).mockReturnValue({ bg: 0x282c34 } as never)
    const pane = buildPane()
    expect(pane.container.style.background).toBe('#282c34')
  })
})
