import React from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import TerminalContextMenu from './TerminalContextMenu'
import type { KeybindingOverrides } from '../../../../shared/keybindings'

type ItemProps = { onSelect?: () => void; disabled?: boolean; children?: React.ReactNode }

const items = vi.hoisted(() => ({ list: [] as ItemProps[] }))
const shortcuts = vi.hoisted(() => ({ list: [] as string[] }))

vi.mock('@/components/ui/dropdown-menu', async () => {
  const React_ = await import('react')
  const passthrough = ({ children }: { children?: React.ReactNode }) =>
    React_.createElement(React_.Fragment, null, children)
  return {
    DropdownMenu: passthrough,
    DropdownMenuContent: passthrough,
    DropdownMenuLabel: passthrough,
    DropdownMenuSeparator: () => null,
    DropdownMenuShortcut: ({ children }: { children?: React.ReactNode }) => {
      shortcuts.list.push(
        React_.Children.toArray(children)
          .filter((child): child is string => typeof child === 'string')
          .join('')
      )
      return React_.createElement(React_.Fragment, null, children)
    },
    DropdownMenuSub: passthrough,
    DropdownMenuSubContent: passthrough,
    DropdownMenuSubTrigger: passthrough,
    DropdownMenuTrigger: passthrough,
    DropdownMenuItem: (props: ItemProps) => {
      items.list.push(props)
      return React.createElement(React.Fragment, null, props.children)
    }
  }
})
vi.mock('@/i18n/i18n', () => ({
  // Interpolates like i18next so labels with {{placeholders}} are assertable.
  translate: (_key: string, fallback: string, values?: Record<string, string>) =>
    Object.entries(values ?? {}).reduce(
      (text, [key, value]) => text.replace(`{{${key}}}`, value),
      fallback
    )
}))
vi.mock('@/lib/agent-catalog', () => ({ AgentIcon: () => null }))
vi.mock('./terminal-context-menu-dismiss', () => ({
  shouldIgnoreTerminalMenuPointerDownOutside: () => false
}))

function childrenText(children: React.ReactNode): string {
  return React.Children.toArray(children)
    .filter((child): child is string => typeof child === 'string')
    .join('')
}

// Why: quick-command labels render inside a truncating <span>, so matching them
// needs the nested text, not just top-level string children.
function deepChildrenText(node: React.ReactNode): string {
  if (typeof node === 'string') {
    return node
  }
  if (Array.isArray(node)) {
    return node.map(deepChildrenText).join('')
  }
  if (React.isValidElement(node)) {
    return deepChildrenText((node.props as { children?: React.ReactNode }).children)
  }
  return ''
}

function renderMenu(overrides: Record<string, unknown> = {}): string {
  const props = {
    open: true,
    onOpenChange: vi.fn(),
    menuPoint: { x: 0, y: 0 },
    menuOpenedAtRef: { current: 0 },
    canClosePane: true,
    canExpandPane: true,
    menuPaneIsExpanded: false,
    onCopy: vi.fn(),
    onPaste: vi.fn(),
    menuSelectionText: '',
    onSearchSelection: vi.fn(),
    linkTargetKind: null,
    onOpenLinkTarget: vi.fn(),
    onCopyLinkTarget: vi.fn(),
    canRevealLinkTarget: false,
    onRevealLinkTarget: vi.fn(),
    canCopyLastCommandOutput: false,
    onCopyLastCommandOutput: vi.fn(),
    onOpenTerminalSettings: vi.fn(),
    canComposeBox: true,
    onComposeBox: vi.fn(),
    onSplitRight: vi.fn(),
    onSplitDown: vi.fn(),
    keybindings: {},
    canEqualizePaneSizes: false,
    onEqualizePaneSizes: vi.fn(),
    onClosePane: vi.fn(),
    onClearScreen: vi.fn(),
    onForkAgentSession: vi.fn(),
    canToggleNativeChat: false,
    isNativeChatView: false,
    onToggleNativeChat: vi.fn(),
    onCopyAgentSessionContext: vi.fn(),
    repoQuickCommands: [],
    globalQuickCommands: [],
    projectQuickCommands: [],
    projectQuickCommandsTrusted: false,
    onReviewProjectQuickCommands: vi.fn(),
    quickCommandRepoLabel: null,
    onQuickCommand: vi.fn(),
    onAddQuickCommand: vi.fn(),
    onToggleExpand: vi.fn(),
    onSetTitle: vi.fn(),
    onClearPaneTitle: vi.fn(),
    canClearPaneTitle: false,
    onCopyTerminalId: vi.fn(),
    onCopyPaneId: vi.fn(),
    ...overrides
  }
  return renderToStaticMarkup(React.createElement(TerminalContextMenu, props))
}

describe('TerminalContextMenu', () => {
  beforeEach(() => {
    items.list = []
    shortcuts.list = []
    vi.stubGlobal('navigator', { userAgent: 'Linux' })
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('renders a "Copy Context" item that triggers onCopyAgentSessionContext (issue #5020)', () => {
    const onCopyAgentSessionContext = vi.fn()
    const onForkAgentSession = vi.fn()
    renderMenu({ onCopyAgentSessionContext, onForkAgentSession })

    const copyContextItem = items.list.find(
      (item) => childrenText(item.children) === 'Copy Context'
    )
    expect(copyContextItem).toBeDefined()

    copyContextItem?.onSelect?.()
    expect(onCopyAgentSessionContext).toHaveBeenCalledTimes(1)
    // Why: copying context must not go through the fork dialog path.
    expect(onForkAgentSession).not.toHaveBeenCalled()
  })

  it('shows one shortcut per terminal menu action on Windows', () => {
    vi.stubGlobal('navigator', {
      userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)'
    })
    const keybindings = {
      'terminal.copySelection': ['Ctrl+Shift+C', 'Ctrl+Insert', 'Ctrl+C'],
      'terminal.splitRight': ['Mod+Shift+D', 'Alt+Shift+Right'],
      'terminal.splitDown': ['Alt+Shift+D', 'Mod+Shift+Minus']
    } satisfies KeybindingOverrides

    renderMenu({ keybindings })

    expect(shortcuts.list).toContain('Ctrl+Shift+C')
    expect(shortcuts.list).toContain('Ctrl+V')
    expect(shortcuts.list).toContain('Ctrl+Shift+D')
    expect(shortcuts.list).toContain('Alt+Shift+D')
    expect(shortcuts.list.some((shortcut) => shortcut.includes(','))).toBe(false)
  })

  const projectCommand = {
    id: 'orcaYaml:repo-1:0',
    label: 'Dev server',
    scope: { type: 'repo', repoId: 'repo-1' },
    action: 'terminal-command',
    command: 'npm run dev',
    appendEnter: true
  }

  it('renders project quick commands under the repo group with provenance and disables them until trusted (#8481)', () => {
    const markup = renderMenu({
      quickCommandRepoLabel: 'My Repo',
      projectQuickCommands: [projectCommand],
      projectQuickCommandsTrusted: false
    })

    const projectItem = items.list.find((item) => deepChildrenText(item.children).includes('Dev server'))
    expect(projectItem).toBeDefined()
    expect(projectItem?.disabled).toBe(true)
    // Why: provenance must be visible — a repo-supplied command may not masquerade as a user-saved one.
    expect(markup).toContain('Defined in orca.yaml')

    const reviewItem = items.list.find(
      (item) => deepChildrenText(item.children).includes('Review orca.yaml trust…')
    )
    expect(reviewItem).toBeDefined()
    expect(reviewItem?.disabled).not.toBe(true)
  })

  it('routes the trust-review hint item to onReviewProjectQuickCommands and closes the menu first', () => {
    const onOpenChange = vi.fn()
    const onReviewProjectQuickCommands = vi.fn()
    const onQuickCommand = vi.fn()
    renderMenu({
      quickCommandRepoLabel: 'My Repo',
      projectQuickCommands: [projectCommand],
      projectQuickCommandsTrusted: false,
      onOpenChange,
      onReviewProjectQuickCommands,
      onQuickCommand
    })

    const reviewItem = items.list.find(
      (item) => deepChildrenText(item.children).includes('Review orca.yaml trust…')
    )
    reviewItem?.onSelect?.()
    expect(onOpenChange).toHaveBeenCalledWith(false)
    expect(onReviewProjectQuickCommands).toHaveBeenCalledTimes(1)
    expect(onQuickCommand).not.toHaveBeenCalled()
  })

  it('shows Search Selection only while the pane has a selection (#9279 A1)', () => {
    const onSearchSelection = vi.fn()
    renderMenu({ menuSelectionText: 'npm ERR! code 1', onSearchSelection })
    const searchItem = items.list.find((item) =>
      deepChildrenText(item.children).includes('Search for')
    )
    expect(searchItem).toBeDefined()
    expect(deepChildrenText(searchItem?.children)).toContain('npm ERR! code 1')
    searchItem?.onSelect?.()
    expect(onSearchSelection).toHaveBeenCalledTimes(1)

    items.list = []
    renderMenu({ menuSelectionText: '' })
    expect(
      items.list.find((item) => deepChildrenText(item.children).includes('Search for'))
    ).toBeUndefined()
  })

  it('ellipsizes a long selection in the Search label but keeps the action', () => {
    renderMenu({ menuSelectionText: 'x'.repeat(60) })
    const searchItem = items.list.find((item) =>
      deepChildrenText(item.children).includes('Search for')
    )
    expect(deepChildrenText(searchItem?.children)).toContain(`${'x'.repeat(24)}…`)
  })

  it('renders Open/Copy link items only when a link target resolved (#9279 A2)', () => {
    renderMenu({})
    expect(
      items.list.find((item) => childrenText(item.children) === 'Open Link')
    ).toBeUndefined()

    items.list = []
    const onOpenLinkTarget = vi.fn()
    const onCopyLinkTarget = vi.fn()
    renderMenu({ linkTargetKind: 'url', onOpenLinkTarget, onCopyLinkTarget })
    const openItem = items.list.find((item) => childrenText(item.children) === 'Open Link')
    const copyItem = items.list.find((item) => childrenText(item.children) === 'Copy Link')
    expect(openItem).toBeDefined()
    expect(copyItem).toBeDefined()
    openItem?.onSelect?.()
    copyItem?.onSelect?.()
    expect(onOpenLinkTarget).toHaveBeenCalledTimes(1)
    expect(onCopyLinkTarget).toHaveBeenCalledTimes(1)
  })

  it('labels file targets Open File / Copy Path', () => {
    renderMenu({ linkTargetKind: 'file' })
    expect(items.list.find((item) => childrenText(item.children) === 'Open File')).toBeDefined()
    expect(items.list.find((item) => childrenText(item.children) === 'Copy Path')).toBeDefined()
    expect(items.list.find((item) => childrenText(item.children) === 'Open Link')).toBeUndefined()
  })

  it('hides Reveal in File Manager for SSH and remote-runtime panes (canRevealLinkTarget=false)', () => {
    renderMenu({ linkTargetKind: 'file', canRevealLinkTarget: false })
    expect(
      items.list.find((item) => childrenText(item.children).startsWith('Reveal in'))
    ).toBeUndefined()

    items.list = []
    const onRevealLinkTarget = vi.fn()
    renderMenu({ linkTargetKind: 'file', canRevealLinkTarget: true, onRevealLinkTarget })
    const revealItem = items.list.find((item) =>
      childrenText(item.children).startsWith('Reveal in')
    )
    expect(revealItem).toBeDefined()
    // Linux userAgent (stubbed in beforeEach) → the generic file-manager label.
    expect(childrenText(revealItem?.children)).toBe('Reveal in File Manager')
    revealItem?.onSelect?.()
    expect(onRevealLinkTarget).toHaveBeenCalledTimes(1)
  })

  it('Copy Last Command Output is hidden when the engine reports no completed block (CM-A3)', () => {
    renderMenu({ canCopyLastCommandOutput: false })
    expect(
      items.list.find((item) => childrenText(item.children) === 'Copy Last Command Output')
    ).toBeUndefined()

    items.list = []
    const onCopyLastCommandOutput = vi.fn()
    renderMenu({ canCopyLastCommandOutput: true, onCopyLastCommandOutput })
    const outputItem = items.list.find(
      (item) => childrenText(item.children) === 'Copy Last Command Output'
    )
    expect(outputItem).toBeDefined()
    outputItem?.onSelect?.()
    expect(onCopyLastCommandOutput).toHaveBeenCalledTimes(1)
  })

  it('Clear Screen & Scrollback routes to onClearScreen (CM-A4 relabel)', () => {
    const onClearScreen = vi.fn()
    renderMenu({ onClearScreen })
    expect(
      items.list.find((item) => childrenText(item.children) === 'Clear Screen')
    ).toBeUndefined()
    const clearItem = items.list.find(
      (item) => childrenText(item.children) === 'Clear Screen & Scrollback'
    )
    expect(clearItem).toBeDefined()
    clearItem?.onSelect?.()
    expect(onClearScreen).toHaveBeenCalledTimes(1)
  })

  it('Terminal Settings item closes the menu first, then routes to onOpenTerminalSettings (CM-A5)', () => {
    const calls: string[] = []
    renderMenu({
      onOpenChange: vi.fn((open: boolean) => calls.push(`openChange:${open}`)),
      onOpenTerminalSettings: vi.fn(() => calls.push('openSettings'))
    })
    const settingsItem = items.list.find(
      (item) => childrenText(item.children) === 'Terminal Settings…'
    )
    expect(settingsItem).toBeDefined()
    settingsItem?.onSelect?.()
    // Why: settings navigation swaps the view; the menu must be force-closed FIRST.
    expect(calls).toEqual(['openChange:false', 'openSettings'])
  })

  it('enables project quick commands once trusted and dispatches through onQuickCommand', () => {
    const onQuickCommand = vi.fn()
    renderMenu({
      quickCommandRepoLabel: 'My Repo',
      projectQuickCommands: [projectCommand],
      projectQuickCommandsTrusted: true,
      onQuickCommand
    })

    expect(
      items.list.find((item) => deepChildrenText(item.children).includes('Review orca.yaml trust…'))
    ).toBeUndefined()
    const projectItem = items.list.find((item) => deepChildrenText(item.children).includes('Dev server'))
    expect(projectItem?.disabled).not.toBe(true)
    projectItem?.onSelect?.()
    expect(onQuickCommand).toHaveBeenCalledWith(projectCommand)
  })
})
