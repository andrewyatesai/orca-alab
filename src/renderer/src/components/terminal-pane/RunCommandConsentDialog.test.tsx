// @vitest-environment happy-dom

import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import RunCommandConsentDialog from './RunCommandConsentDialog'
import {
  getPendingRunCommandConsent,
  requestRunCommandConsent,
  resetRunCommandConsentForTest
} from '@/lib/deep-link-consent-gate'
import { runDeepLinkCommandInNewTab } from '@/lib/deep-link-run-command'
import type { OrcaDeepLinkOrigin } from '../../../../shared/orca-deep-link'

const mockStoreState = vi.hoisted(() => ({
  state: {
    getKnownWorktreeById: vi.fn((id: string) =>
      id === 'repo::wt'
        ? { id: 'repo::wt', displayName: 'my-feature', path: '/repos/my-feature', hostId: 'local' }
        : id === 'repo::origin-wt'
          ? { id: 'repo::origin-wt', displayName: 'origin-pane', path: '/repos/origin' }
          : undefined
    )
  }
}))

vi.mock('@/store', () => {
  const useAppStore = (selector: (state: unknown) => unknown): unknown =>
    selector(mockStoreState.state)
  useAppStore.getState = () => mockStoreState.state
  return { useAppStore }
})

vi.mock('@/lib/deep-link-run-command', () => ({
  runDeepLinkCommandInNewTab: vi.fn(() => ({ tabId: 'tab-new' }))
}))

vi.mock('@/lib/deep-link-ui-notices', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  showDeepLinkUnknownWorkspaceToast: vi.fn()
}))

const mountedRoots: Root[] = []

async function renderDialog(): Promise<void> {
  const container = document.createElement('div')
  document.body.appendChild(container)
  const root = createRoot(container)
  mountedRoots.push(root)
  await act(async () => {
    root.render(<RunCommandConsentDialog />)
  })
}

async function openConsent(origin: OrcaDeepLinkOrigin, command = 'npm run build'): Promise<void> {
  await act(async () => {
    requestRunCommandConsent({
      link: { kind: 'run', worktreeId: 'repo::wt', command },
      origin
    })
  })
}

function findButton(label: string): HTMLButtonElement {
  const button = [...document.body.querySelectorAll<HTMLButtonElement>('button')].find(
    (candidate) => candidate.textContent === label
  )
  if (!button) {
    throw new Error(`Button not found: ${label}`)
  }
  return button
}

beforeEach(() => {
  vi.clearAllMocks()
  resetRunCommandConsentForTest()
})

afterEach(async () => {
  for (const root of mountedRoots) {
    await act(async () => root.unmount())
  }
  mountedRoots.length = 0
  document.body.innerHTML = ''
})

describe('RunCommandConsentDialog', () => {
  it('renders full command and worktree', async () => {
    await renderDialog()
    await openConsent({ source: 'os' }, 'npm run build && echo done')

    const text = document.body.textContent ?? ''
    expect(text).toContain('npm run build && echo done')
    expect(text).toContain('my-feature')
    expect(text).toContain('/repos/my-feature')
  })

  it('shows the execution host so "Run command" is never ambiguous about where', async () => {
    await renderDialog()
    await openConsent({ source: 'os' })

    expect(document.body.textContent).toContain('this computer')
  })

  it('labels terminal origin as untrusted output', async () => {
    await renderDialog()
    await openConsent({ source: 'terminal', worktreeId: 'repo::origin-wt' })

    const text = document.body.textContent ?? ''
    expect(text).toContain('origin-pane')
    expect(text).toContain('untrusted')
  })

  it('labels os origin as coming from outside Orca', async () => {
    await renderDialog()
    await openConsent({ source: 'os' })

    expect(document.body.textContent).toContain('outside Orca')
  })

  it('no always-allow control rendered', async () => {
    await renderDialog()
    await openConsent({ source: 'os' })

    expect(document.body.querySelector('input[type="checkbox"]')).toBeNull()
    expect(document.body.querySelector('[role="checkbox"]')).toBeNull()
    expect(document.body.textContent?.toLowerCase()).not.toContain('always')
    expect(document.body.textContent?.toLowerCase()).not.toContain("don't ask")
  })

  it('enter key does not confirm', async () => {
    await renderDialog()
    await openConsent({ source: 'os' })

    const dialog = document.body.querySelector('[role="dialog"]')
    expect(dialog).not.toBeNull()
    await act(async () => {
      dialog?.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })
      )
    })

    expect(runDeepLinkCommandInNewTab).not.toHaveBeenCalled()
    // Consent must still be open — Enter neither confirms nor dismisses.
    expect(getPendingRunCommandConsent()).not.toBeNull()
  })

  it('cancel settles without running anything', async () => {
    await renderDialog()
    await openConsent({ source: 'os' })

    await act(async () => {
      findButton('Cancel').click()
    })

    expect(runDeepLinkCommandInNewTab).not.toHaveBeenCalled()
    expect(getPendingRunCommandConsent()).toBeNull()
  })

  it('confirm runs the command in a new tab and settles', async () => {
    await renderDialog()
    await openConsent({ source: 'os' }, 'npm test')

    await act(async () => {
      findButton('Run command').click()
    })

    expect(runDeepLinkCommandInNewTab).toHaveBeenCalledWith({
      kind: 'run',
      worktreeId: 'repo::wt',
      command: 'npm test'
    })
    expect(getPendingRunCommandConsent()).toBeNull()
  })
})
