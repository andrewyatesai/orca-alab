import { createElement } from 'react'
import { act, create, type ReactTestRenderer } from 'react-test-renderer'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { WorktreeFilterDrawer } from './WorktreeFilterDrawer'
import type { FilterState } from '../worktree/workspace-list-types'
import type { WorkspaceSectionRepo } from '../worktree/use-workspace-sections'

vi.mock('react-native', () => ({
  Pressable: 'Pressable',
  StyleSheet: { create: <T>(styles: T) => styles, hairlineWidth: 1 },
  Text: 'Text',
  View: 'View'
}))

vi.mock('lucide-react-native', () => ({ Check: 'Check' }))

// BottomDrawer renders its children only while visible, so mirror that so the
// test exercises the same mount/unmount gating the real drawer relies on.
vi.mock('./BottomDrawer', () => ({
  BottomDrawer: ({ visible, children }: { visible: boolean; children: unknown }) =>
    visible ? children : null
}))

vi.mock('../theme/mobile-theme', () => ({
  colors: {
    textPrimary: '#fff',
    textSecondary: '#ccc',
    textMuted: '#999',
    bgPanel: '#111',
    borderSubtle: '#222'
  },
  spacing: { xs: 4, sm: 8, md: 12, lg: 16 },
  typography: { bodySize: 14 }
}))

const baseFilters: FilterState = {
  filterRepoIds: new Set<string>(),
  hideSleeping: false,
  hideDefaultBranch: false
}

const repos: WorkspaceSectionRepo[] = [
  { id: 'r1', name: 'alpha', color: '#111' },
  { id: 'r2', name: 'beta', color: '#222' }
]

function noop() {}

type DrawerProps = Parameters<typeof WorktreeFilterDrawer>[0]

function renderDrawer(overrides: Partial<DrawerProps>): ReactTestRenderer {
  const props: DrawerProps = {
    visible: true,
    onClose: noop,
    activeFilterCount: 0,
    onClearFilters: noop,
    filters: baseFilters,
    onToggleHideSleeping: noop,
    onToggleHideDefaultBranch: noop,
    uniqueRepos: repos,
    onToggleRepoFilter: noop,
    ...overrides
  }
  let tree: ReactTestRenderer
  act(() => {
    tree = create(createElement(WorktreeFilterDrawer, props))
  })
  return tree!
}

function pressables(tree: ReactTestRenderer) {
  return tree.root.findAll((n) => n.type === 'Pressable')
}

afterEach(() => vi.clearAllMocks())

describe('WorktreeFilterDrawer', () => {
  it('renders nothing while hidden', () => {
    expect(renderDrawer({ visible: false }).toJSON()).toBeNull()
  })

  it('shows the clear-filters control only when filters are active', () => {
    // No active filters: two workspace toggles + two repo rows = 4 Pressables.
    expect(pressables(renderDrawer({ activeFilterCount: 0 }))).toHaveLength(4)

    const onClearFilters = vi.fn()
    const tree = renderDrawer({ activeFilterCount: 2, onClearFilters })
    const rows = pressables(tree)
    expect(rows).toHaveLength(5)
    act(() => rows[0].props.onPress())
    expect(onClearFilters).toHaveBeenCalledTimes(1)
  })

  it('wires the workspace toggles and per-repo filter callback', () => {
    const onToggleHideSleeping = vi.fn()
    const onToggleHideDefaultBranch = vi.fn()
    const onToggleRepoFilter = vi.fn()
    const tree = renderDrawer({
      onToggleHideSleeping,
      onToggleHideDefaultBranch,
      onToggleRepoFilter
    })
    const [hideSleeping, hideDefault, repoOne, repoTwo] = pressables(tree)
    act(() => hideSleeping.props.onPress())
    act(() => hideDefault.props.onPress())
    act(() => repoOne.props.onPress())
    act(() => repoTwo.props.onPress())
    expect(onToggleHideSleeping).toHaveBeenCalledTimes(1)
    expect(onToggleHideDefaultBranch).toHaveBeenCalledTimes(1)
    expect(onToggleRepoFilter).toHaveBeenNthCalledWith(1, 'r1')
    expect(onToggleRepoFilter).toHaveBeenNthCalledWith(2, 'r2')
  })

  it('hides the repositories section when only one repo is present', () => {
    // Only the two workspace toggle rows remain; no repo rows rendered.
    expect(pressables(renderDrawer({ uniqueRepos: [repos[0]] }))).toHaveLength(2)
  })
})
