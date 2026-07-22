// @vitest-environment happy-dom

import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/i18n/i18n', () => ({
  translate: (_key: string, fallback: string) => fallback
}))

const sendMock = vi.hoisted(() => vi.fn())
vi.mock('./terminal-compose-box-send', () => ({
  sendComposeBoxDraft: sendMock
}))

import { TerminalComposeBox } from './TerminalComposeBox'
import {
  getComposeBoxDraftEntry,
  resetComposeBoxDraftCacheForTests,
  setComposeBoxDraftEntry
} from './terminal-compose-box-draft-cache'

type TestPane = {
  id: number
  leafId: string
  terminal: { focus: ReturnType<typeof vi.fn>; modes: { bracketedPasteMode: boolean } }
  atermController: null
}

function makePane(options: { bracketed?: boolean } = {}): TestPane {
  return {
    id: 1,
    leafId: 'leaf-1',
    terminal: { focus: vi.fn(), modes: { bracketedPasteMode: options.bracketed ?? false } },
    atermController: null
  }
}

let container: HTMLDivElement | null = null
let root: Root | null = null

function renderBox(
  overrides: { paneKey?: string; pane?: TestPane; onClose?: () => void } = {}
): void {
  const pane = overrides.pane ?? makePane()
  if (!container) {
    container = document.createElement('div')
    document.body.appendChild(container)
    root = createRoot(container)
  }
  act(() => {
    root!.render(
      <TerminalComposeBox
        key={overrides.paneKey ?? 'tab-1:leaf-A'}
        paneKey={overrides.paneKey ?? 'tab-1:leaf-A'}
        pane={pane as never}
        transport={null}
        tabId="tab-1"
        worktreeId="wt-1"
        forceBracketedMultilineTextPaste={false}
        keybindings={{}}
        terminalShortcutPolicy="orca-first"
        getManager={() => null}
        getPaneTransports={() => new Map()}
        onClose={overrides.onClose ?? (() => undefined)}
      />
    )
  })
}

function unmountBox(): void {
  if (root) {
    act(() => root!.unmount())
    root = null
  }
  container?.remove()
  container = null
}

function textarea(): HTMLTextAreaElement {
  return container!.querySelector('textarea')!
}

function typeDraft(text: string): void {
  const element = textarea()
  const setValue = Object.getOwnPropertyDescriptor(
    window.HTMLTextAreaElement.prototype,
    'value'
  )!.set!
  act(() => {
    setValue.call(element, text)
    element.dispatchEvent(new Event('input', { bubbles: true }))
  })
}

function pressKey(init: KeyboardEventInit): KeyboardEvent {
  const event = new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init })
  act(() => {
    textarea().dispatchEvent(event)
  })
  return event
}

beforeEach(() => {
  resetComposeBoxDraftCacheForTests()
  sendMock.mockReset()
  sendMock.mockResolvedValue({ status: 'sent', submitted: true })
  vi.stubGlobal('navigator', { userAgent: 'Macintosh' })
})

afterEach(() => {
  unmountBox()
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

describe('TerminalComposeBox', () => {
  it('restores the per-pane draft and history across close/reopen and pane switches', () => {
    renderBox({ paneKey: 'tab-1:leaf-A' })
    typeDraft('echo hello')
    unmountBox()

    renderBox({ paneKey: 'tab-1:leaf-A' })
    expect(textarea().value).toBe('echo hello')
    unmountBox()

    // A different pane's box starts from its own (empty) cached draft.
    renderBox({ paneKey: 'tab-1:leaf-B' })
    expect(textarea().value).toBe('')
  })

  it('gates ArrowUp history recall on empty draft or active recall', () => {
    setComposeBoxDraftEntry('tab-1:leaf-A', {
      draft: 'typed',
      history: { entries: ['prev command'], index: null }
    })
    renderBox({ paneKey: 'tab-1:leaf-A' })

    // Non-empty draft without an active recall: ArrowUp edits lines, not history.
    pressKey({ key: 'ArrowUp' })
    expect(textarea().value).toBe('typed')

    typeDraft('')
    pressKey({ key: 'ArrowUp' })
    expect(textarea().value).toBe('prev command')

    // Active recall: ArrowDown steps forward (back to the empty draft).
    pressKey({ key: 'ArrowDown' })
    expect(textarea().value).toBe('')
  })

  it('Esc closes, keeps the draft, and does not propagate', () => {
    const onClose = vi.fn()
    const bodyListener = vi.fn()
    document.body.addEventListener('keydown', bodyListener)
    renderBox({ paneKey: 'tab-1:leaf-A', onClose })
    typeDraft('keep me')

    pressKey({ key: 'Escape' })

    expect(onClose).toHaveBeenCalledTimes(1)
    expect(getComposeBoxDraftEntry('tab-1:leaf-A').draft).toBe('keep me')
    expect(bodyListener).not.toHaveBeenCalled()
    document.body.removeEventListener('keydown', bodyListener)
  })

  it('shows the sequential-run warning only for multiline drafts into no-2004 panes', () => {
    setComposeBoxDraftEntry('tab-1:leaf-A', {
      draft: 'echo 1\necho 2',
      history: { entries: [], index: null }
    })
    renderBox({ paneKey: 'tab-1:leaf-A', pane: makePane({ bracketed: false }) })
    expect(container!.textContent).toContain('run one by one')
    unmountBox()

    renderBox({ paneKey: 'tab-1:leaf-A', pane: makePane({ bracketed: true }) })
    expect(container!.textContent).not.toContain('run one by one')
    unmountBox()

    // Single-line drafts never warn, negotiated or not.
    setComposeBoxDraftEntry('tab-1:leaf-A', {
      draft: 'echo 1',
      history: { entries: [], index: null }
    })
    renderBox({ paneKey: 'tab-1:leaf-A', pane: makePane({ bracketed: false }) })
    expect(container!.textContent).not.toContain('run one by one')
  })

  it("closes on its own toggle chord from inside the box, including the shifted '>' key shape", () => {
    const onClose = vi.fn()
    renderBox({ paneKey: 'tab-1:leaf-A', onClose })
    typeDraft('still drafting')

    // Why: on most layouts Shift+Period reports key '>'; the shared normalizer must still match.
    pressKey({ key: '>', code: 'Period', metaKey: true, shiftKey: true })

    expect(onClose).toHaveBeenCalledTimes(1)
    expect(getComposeBoxDraftEntry('tab-1:leaf-A').draft).toBe('still drafting')
  })

  it('Enter sends the draft, pushes history, and closes', async () => {
    const onClose = vi.fn()
    renderBox({ paneKey: 'tab-1:leaf-A', onClose })
    typeDraft('echo done')

    pressKey({ key: 'Enter' })
    await act(async () => {})

    expect(sendMock).toHaveBeenCalledTimes(1)
    expect(sendMock.mock.calls[0][0]).toMatchObject({ text: 'echo done', mode: 'submit' })
    expect(onClose).toHaveBeenCalledTimes(1)
    expect(getComposeBoxDraftEntry('tab-1:leaf-A')).toMatchObject({
      draft: '',
      history: { entries: ['echo done'], index: null }
    })
  })

  it('Mod+Enter stages the body without submitting', async () => {
    renderBox({ paneKey: 'tab-1:leaf-A' })
    typeDraft('echo staged')

    pressKey({ key: 'Enter', metaKey: true })
    await act(async () => {})

    expect(sendMock).toHaveBeenCalledTimes(1)
    expect(sendMock.mock.calls[0][0]).toMatchObject({ text: 'echo staged', mode: 'stage' })
  })

  it('keeps the draft and surfaces the reason when the send is rejected', async () => {
    sendMock.mockResolvedValue({ status: 'rejected', reason: 'payload-too-large' })
    const onClose = vi.fn()
    renderBox({ paneKey: 'tab-1:leaf-A', onClose })
    typeDraft('echo huge')

    pressKey({ key: 'Enter' })
    await act(async () => {})

    expect(onClose).not.toHaveBeenCalled()
    expect(getComposeBoxDraftEntry('tab-1:leaf-A').draft).toBe('echo huge')
    expect(container!.textContent).toContain('too large')
  })

  it('never submits mid-composition and absorbs the IME re-dispatched Enter (composition-mid-send repro)', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(10_000)
    renderBox({ paneKey: 'tab-1:leaf-A' })
    typeDraft('안녕하세요')

    // Composing: the commit Enter must not submit.
    act(() => {
      textarea().dispatchEvent(new Event('compositionstart', { bubbles: true }))
    })
    pressKey({ key: 'Enter' })
    expect(sendMock).not.toHaveBeenCalled()

    // macOS Hangul re-dispatches the committing Enter as a plain keydown just after compositionend.
    act(() => {
      textarea().dispatchEvent(new Event('compositionend', { bubbles: true }))
    })
    vi.setSystemTime(10_002)
    pressKey({ key: 'Enter' })
    expect(sendMock).not.toHaveBeenCalled()

    // A genuinely separate Enter after the absorb window submits.
    vi.setSystemTime(10_200)
    pressKey({ key: 'Enter' })
    expect(sendMock).toHaveBeenCalledTimes(1)
  })
})
