/**
 * @vitest-environment happy-dom
 *
 * Issue #9338 — remapping a key while a CJK IME is active.
 *
 * With a macOS CJK IME in full-width punctuation mode, pressing the physical
 * Period key reports `key: '。', code: 'Period', isComposing: false` and no
 * composition ever opens. A custom sendText entry bound to bare `Period` with
 * `matchPhysicalKey: true` must intercept it: the transport receives the
 * remapped ASCII `.` and the IME's direct-inserted `。` (delivered via a
 * `beforeinput` that ignores the canceled keydown default) is swallowed.
 * The same keystroke arriving mid-composition must pass through untouched.
 *
 * Re-run:
 *   pnpm exec vitest run --config config/vitest.config.ts \
 *     src/renderer/src/components/terminal-pane/repro-9338-cjk-fullwidth-punctuation.test.ts
 */
import { renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ResolvedCustomKeybinding } from '../../../../shared/custom-keybindings'
import { useTerminalKeyboardShortcuts } from './keyboard-handlers'
import type { PtyTransport } from './pty-transport'

const remapEntry: ResolvedCustomKeybinding = {
  id: 'custom.k3v9x2m1q8za',
  title: 'ASCII period (CJK remap)',
  action: { type: 'sendText', text: '.' },
  bindings: ['Period'],
  matchPhysicalKey: true,
  decodedText: '.'
}

function createPane() {
  return {
    id: 1,
    leafId: 'leaf-1',
    terminal: {
      getSelection: () => '',
      focus: vi.fn(),
      element: document.createElement('div')
    },
    atermController: null
  }
}

function mountShortcuts(pane: unknown, sendInput: ReturnType<typeof vi.fn>) {
  const transports = new Map<number, PtyTransport>([[1, { sendInput } as unknown as PtyTransport]])
  const noop = (): void => undefined
  return renderHook(() =>
    useTerminalKeyboardShortcuts({
      tabId: 'tab-1',
      worktreeId: 'wt-1',
      isActive: true,
      keyboardScopeRef: { current: null },
      managerRef: {
        current: {
          getActivePane: () => pane,
          getPanes: () => [pane]
        } as never
      },
      paneTransportsRef: { current: transports },
      paneCwdRef: { current: new Map() },
      fallbackCwd: '/tmp',
      expandedPaneIdRef: { current: null },
      setExpandedPane: noop,
      restoreExpandedLayout: noop,
      refreshPaneSizes: noop,
      persistLayoutSnapshot: noop,
      toggleExpandPane: noop,
      setSearchOpen: noop as never,
      onToggleComposeBox: noop,
      onSearchSelectedText: noop,
      onRequestClosePane: noop,
      onClearPaneScrollback: noop,
      onSetTitle: noop,
      onClearPaneTitle: noop,
      searchOpenRef: { current: false },
      searchStateRef: { current: { query: '', caseSensitive: false, regex: false } },
      macOptionAsAltRef: { current: 'false' as const },
      customKeybindings: [remapEntry]
    })
  )
}

describe('issue #9338 CJK full-width punctuation remap', () => {
  beforeEach(() => {
    vi.stubGlobal('navigator', { userAgent: 'Macintosh' })
  })
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('sends the remapped ASCII byte and swallows the companion 。 beforeinput', () => {
    const sendInput = vi.fn(() => true)
    const { unmount } = mountShortcuts(createPane(), sendInput)

    const keydown = new KeyboardEvent('keydown', {
      key: '。',
      code: 'Period',
      bubbles: true,
      cancelable: true
    })
    window.dispatchEvent(keydown)

    expect(sendInput).toHaveBeenCalledWith('.')
    expect(keydown.defaultPrevented).toBe(true)

    // IME direct-insert path: beforeinput carrying the full-width char must be swallowed…
    const companion = new InputEvent('beforeinput', {
      data: '。',
      inputType: 'insertText',
      bubbles: true,
      cancelable: true
    })
    window.dispatchEvent(companion)
    expect(companion.defaultPrevented).toBe(true)

    // …while unrelated text insertion in the same window stays intact.
    const unrelated = new InputEvent('beforeinput', {
      data: 'x',
      inputType: 'insertText',
      bubbles: true,
      cancelable: true
    })
    window.dispatchEvent(unrelated)
    expect(unrelated.defaultPrevented).toBe(false)

    // Keyup disarms the suppression.
    window.dispatchEvent(new KeyboardEvent('keyup', { key: '。', code: 'Period', bubbles: true }))
    const afterRelease = new InputEvent('beforeinput', {
      data: '。',
      inputType: 'insertText',
      bubbles: true,
      cancelable: true
    })
    window.dispatchEvent(afterRelease)
    expect(afterRelease.defaultPrevented).toBe(false)
    unmount()
  })

  it('sends nothing and leaves the event untouched while composing', () => {
    const sendInput = vi.fn(() => true)
    const { unmount } = mountShortcuts(createPane(), sendInput)

    const keydown = new KeyboardEvent('keydown', {
      key: '。',
      code: 'Period',
      isComposing: true,
      bubbles: true,
      cancelable: true
    })
    window.dispatchEvent(keydown)

    expect(sendInput).not.toHaveBeenCalled()
    expect(keydown.defaultPrevented).toBe(false)
    unmount()
  })
})
