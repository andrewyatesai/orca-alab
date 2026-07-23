// @vitest-environment happy-dom

import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { SESSION_RESTORED_BANNER_TEXT } from './SessionRestoredBanner'
import { SessionRestoredBannerPortals } from './SessionRestoredBannerPortals'
import {
  addSessionRestoredBannerPane,
  dismissSessionRestoredBannerPanes,
  offerableRestoredLastCommand,
  pruneSessionRestoredBannerPanes,
  removeSessionRestoredBannerPane,
  seedStartupSessionRestoredBanner,
  syncSessionRestoredBannerTitleSpace,
  type SessionRestoredBannerPane,
  type SessionRestoredBannerState
} from './session-restored-banner-pane-state'

const mountedRoots: Root[] = []

function createPane(id: number): SessionRestoredBannerPane {
  const container = document.createElement('div')
  container.className = 'pane'
  container.dataset.leafId = `leaf-${id}`
  document.body.appendChild(container)
  // Only the paste/focus seam is exercised by the banner affordance.
  const terminal = { paste: vi.fn(), focus: vi.fn() } as unknown as SessionRestoredBannerPane['terminal']
  return { id, container, terminal }
}

function states(
  ...entries: [number, SessionRestoredBannerState][]
): Map<number, SessionRestoredBannerState> {
  return new Map(entries)
}

function bannerOnly(...paneIds: number[]): Map<number, SessionRestoredBannerState> {
  return states(...paneIds.map((id): [number, SessionRestoredBannerState] => [id, { lastCommand: null }]))
}

async function renderPortals(
  panes: readonly SessionRestoredBannerPane[],
  bannerStates: ReadonlyMap<number, SessionRestoredBannerState>,
  onTypeItAgain: (pane: SessionRestoredBannerPane, command: string) => void = () => {}
): Promise<void> {
  const rootContainer = document.createElement('div')
  document.body.appendChild(rootContainer)
  const root = createRoot(rootContainer)
  mountedRoots.push(root)
  await act(async () => {
    root.render(
      <SessionRestoredBannerPortals panes={panes} states={bannerStates} onTypeItAgain={onTypeItAgain} />
    )
  })
}

function eventFrom(target: HTMLElement, event: KeyboardEvent | PointerEvent): typeof event {
  target.dispatchEvent(event)
  return event
}

function paneText(pane: SessionRestoredBannerPane): string {
  return pane.container.textContent ?? ''
}

describe('session restored banner pane state', () => {
  afterEach(async () => {
    await act(async () => {
      for (const root of mountedRoots.splice(0)) {
        root.unmount()
      }
    })
    document.body.innerHTML = ''
  })

  it('seeds sidebar startup onto the created pane and renders its overlay there', async () => {
    const firstPane = createPane(1)
    const createdPane = createPane(2)
    let bannerStates = states()

    seedStartupSessionRestoredBanner(
      { showSessionRestoredBanner: true },
      createdPane.id,
      (paneId) => {
        bannerStates = addSessionRestoredBannerPane(bannerStates, paneId)
      }
    )
    await renderPortals([firstPane, createdPane], bannerStates)

    expect([...bannerStates.keys()]).toEqual([createdPane.id])
    expect(paneText(firstPane)).toBe('')
    expect(paneText(createdPane)).toBe(SESSION_RESTORED_BANNER_TEXT)
  })

  it('does not reserve title space for chromeless always-on pane headers', () => {
    const activePane = createPane(1)
    const secondPane = createPane(2)

    const needsFit = syncSessionRestoredBannerTitleSpace({
      panes: [activePane, secondPane],
      paneTitles: {},
      renamingPaneId: null,
      sessionRestoredBannerPanes: states()
    })

    expect(needsFit).toBe(false)
    expect(activePane.container.hasAttribute('data-has-title')).toBe(false)
    expect(secondPane.container.hasAttribute('data-has-title')).toBe(false)
  })

  it('reserves title space for explicit titles and inline rename', () => {
    const titledPane = createPane(1)
    const renamingPane = createPane(2)

    const needsFit = syncSessionRestoredBannerTitleSpace({
      panes: [titledPane, renamingPane],
      paneTitles: { [titledPane.id]: 'server' },
      renamingPaneId: renamingPane.id,
      sessionRestoredBannerPanes: states()
    })

    expect(needsFit).toBe(true)
    expect(titledPane.container.hasAttribute('data-has-title')).toBe(true)
    expect(renamingPane.container.hasAttribute('data-has-title')).toBe(true)
  })

  it('renders and reserves title space only on the restored inactive split pane', async () => {
    const activePane = createPane(1)
    const inactiveRestoredPane = createPane(2)
    const bannerStates = bannerOnly(inactiveRestoredPane.id)

    const needsFit = syncSessionRestoredBannerTitleSpace({
      panes: [activePane, inactiveRestoredPane],
      paneTitles: {},
      renamingPaneId: null,
      sessionRestoredBannerPanes: bannerStates
    })
    await renderPortals([activePane, inactiveRestoredPane], bannerStates)

    expect(needsFit).toBe(true)
    expect(activePane.container.hasAttribute('data-has-title')).toBe(false)
    expect(inactiveRestoredPane.container.hasAttribute('data-has-title')).toBe(true)
    expect(paneText(activePane)).toBe('')
    expect(paneText(inactiveRestoredPane)).toBe(SESSION_RESTORED_BANNER_TEXT)
  })

  it('dismisses only the interacted pane for pointer and key events', () => {
    const firstPane = createPane(1)
    const secondPane = createPane(2)
    const firstChild = document.createElement('button')
    const secondChild = document.createElement('button')
    firstPane.container.appendChild(firstChild)
    secondPane.container.appendChild(secondChild)

    const afterPointer = dismissSessionRestoredBannerPanes(
      bannerOnly(firstPane.id, secondPane.id),
      eventFrom(secondChild, new PointerEvent('pointerdown', { bubbles: true })),
      [firstPane, secondPane]
    )
    const afterKey = dismissSessionRestoredBannerPanes(
      bannerOnly(firstPane.id, secondPane.id),
      eventFrom(firstChild, new KeyboardEvent('keydown', { bubbles: true })),
      [firstPane, secondPane]
    )

    expect([...afterPointer.keys()]).toEqual([firstPane.id])
    expect([...afterKey.keys()]).toEqual([secondPane.id])
  })

  it('clears all restored banners when dismissal cannot resolve a pane', () => {
    const firstPane = createPane(1)
    const secondPane = createPane(2)
    const outside = document.createElement('button')
    document.body.appendChild(outside)

    const afterDismiss = dismissSessionRestoredBannerPanes(
      bannerOnly(firstPane.id, secondPane.id),
      eventFrom(outside, new PointerEvent('pointerdown', { bubbles: true })),
      [firstPane, secondPane]
    )

    expect(afterDismiss.size).toBe(0)
  })

  it('clears banners for closed or removed panes', () => {
    const firstPane = createPane(1)
    const secondPane = createPane(2)

    expect([
      ...removeSessionRestoredBannerPane(bannerOnly(firstPane.id, secondPane.id), 2).keys()
    ]).toEqual([firstPane.id])
    expect([
      ...pruneSessionRestoredBannerPanes(bannerOnly(firstPane.id, secondPane.id), [firstPane]).keys()
    ]).toEqual([firstPane.id])
  })

  it('keeps a recorded lastCommand when a later null trigger re-adds the pane', () => {
    const withCommand = addSessionRestoredBannerPane(states(), 1, 'npm run dev')
    const afterNullTrigger = addSessionRestoredBannerPane(withCommand, 1, null)
    expect(afterNullTrigger.get(1)).toEqual({ lastCommand: 'npm run dev' })
  })

  // #7596: the affordance types the command WITHOUT executing.
  it('Type it again pastes the command without a trailing newline and dismisses', async () => {
    const pane = createPane(1)
    const bannerStates = states([pane.id, { lastCommand: 'npm run dev' }])
    const onTypeItAgain = vi.fn((target: SessionRestoredBannerPane, command: string) => {
      target.terminal.paste(command)
    })

    await renderPortals([pane], bannerStates, onTypeItAgain)

    expect(paneText(pane)).toContain('npm run dev')
    const action = pane.container.querySelector<HTMLButtonElement>(
      '[data-session-restored-banner-action]'
    )
    expect(action).not.toBeNull()
    await act(async () => {
      action!.click()
    })
    expect(onTypeItAgain).toHaveBeenCalledWith(pane, 'npm run dev')
    expect(pane.terminal.paste).toHaveBeenCalledTimes(1)
    expect(pane.terminal.paste).toHaveBeenCalledWith('npm run dev')
    // No variant with a trailing newline/carriage return ever reaches the pane.
    expect(vi.mocked(pane.terminal.paste).mock.calls[0][0]).not.toMatch(/[\r\n]/)
  })

  // Why: the dismiss listener is capture-phase pointerdown; without the action
  // exemption the banner would unmount before its own click could fire.
  it('pointerdown on the action button does not dismiss the banner', async () => {
    const pane = createPane(1)
    const bannerStates = states([pane.id, { lastCommand: 'npm run dev' }])
    await renderPortals([pane], bannerStates)

    const action = pane.container.querySelector<HTMLButtonElement>(
      '[data-session-restored-banner-action]'
    )!
    const afterPointer = dismissSessionRestoredBannerPanes(
      bannerStates,
      eventFrom(action, new PointerEvent('pointerdown', { bubbles: true })),
      [pane]
    )
    expect([...afterPointer.keys()]).toEqual([pane.id])
  })
})

describe('offerableRestoredLastCommand', () => {
  it('accepts a plain single-line command', () => {
    expect(offerableRestoredLastCommand('npm run dev')).toBe('npm run dev')
    expect(offerableRestoredLastCommand('  npm run dev  ')).toBe('npm run dev')
  })

  it('rejects empty, multiline, and oversized commands', () => {
    expect(offerableRestoredLastCommand(undefined)).toBeNull()
    expect(offerableRestoredLastCommand(null)).toBeNull()
    expect(offerableRestoredLastCommand('   ')).toBeNull()
    expect(offerableRestoredLastCommand('a\nb')).toBeNull()
    expect(offerableRestoredLastCommand('a\rb')).toBeNull()
    expect(offerableRestoredLastCommand('x'.repeat(201))).toBeNull()
    expect(offerableRestoredLastCommand('x'.repeat(200))).toBe('x'.repeat(200))
  })
})
