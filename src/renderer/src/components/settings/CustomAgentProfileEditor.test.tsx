// @vitest-environment happy-dom

import React, { act, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { describe, expect, it, vi } from 'vitest'

// Keep the component isolated from i18n init and the full agent catalog.
vi.mock('@/i18n/i18n', () => ({ translate: (_key: string, fallback: string) => fallback }))
vi.mock('@/lib/agent-catalog', () => ({
  getAgentCatalog: () => [{ id: 'claude', label: 'Claude', cmd: 'claude' }]
}))

import { CustomAgentProfileEditor } from './CustomAgentProfileEditor'
import { newCustomAgentEnvPair, type CustomAgentDraftRow } from './custom-agent-profile-draft'

function makeDraft(): CustomAgentDraftRow {
  return {
    id: 'agent-1',
    label: 'Agent',
    baseAgent: 'claude' as CustomAgentDraftRow['baseAgent'],
    command: 'claude',
    envPairs: [
      { id: 'pair-0', key: 'K0', value: 'v0' },
      { id: 'pair-1', key: 'K1', value: 'v1' },
      { id: 'pair-2', key: 'K2', value: 'v2' }
    ]
  }
}

/** Stateful wrapper mirroring CustomAgentsSection: owns the draft, applies patches. */
function Harness(): React.JSX.Element {
  const [draft, setDraft] = useState<CustomAgentDraftRow>(makeDraft)
  return (
    <CustomAgentProfileEditor
      draft={draft}
      onDraftChange={(patch) => setDraft((prev) => ({ ...prev, ...patch }))}
      onSave={() => {}}
    />
  )
}

function valueInputs(container: HTMLElement): HTMLInputElement[] {
  // value inputs carry placeholder 'value'; key inputs carry placeholder 'KEY'.
  return Array.from(container.querySelectorAll<HTMLInputElement>('input[placeholder="value"]'))
}

describe('CustomAgentProfileEditor env-var rows', () => {
  it('preserves focus/identity of surviving rows when an earlier row is deleted', () => {
    const container = document.createElement('div')
    document.body.appendChild(container)
    const root = createRoot(container)
    act(() => root.render(<Harness />))

    // Focus the LAST row's value input (the one whose physical node an index-key
    // reconcile would unmount when the list shrinks).
    const lastValueInput = valueInputs(container)[2]
    expect(lastValueInput.value).toBe('v2')
    lastValueInput.focus()
    expect(document.activeElement).toBe(lastValueInput)

    // Delete the FIRST row via its trash button.
    const trashButtons = Array.from(container.querySelectorAll('button')).filter((b) =>
      b.getAttribute('title')?.includes('Remove env var')
    )
    expect(trashButtons).toHaveLength(3)
    act(() => {
      trashButtons[0].dispatchEvent(new MouseEvent('click', { bubbles: true }))
    })

    // The two surviving rows now show K1/v1 and K2/v2.
    const survivors = valueInputs(container)
    expect(survivors.map((i) => i.value)).toEqual(['v1', 'v2'])

    // The exact DOM node that held focus must survive and stay focused — proving
    // the row was keyed by identity, not array index (an index key would unmount
    // the last physical node, dropping focus/caret onto the wrong logical row).
    expect(lastValueInput.isConnected).toBe(true)
    expect(lastValueInput.value).toBe('v2')
    expect(document.activeElement).toBe(lastValueInput)

    act(() => root.unmount())
    container.remove()
  })

  it('gives every env pair a stable unique id via the factory', () => {
    const a = newCustomAgentEnvPair('K', 'v')
    const b = newCustomAgentEnvPair()
    expect(a.id).toBeTruthy()
    expect(a.id).not.toBe(b.id)
    expect(a).toMatchObject({ key: 'K', value: 'v' })
    expect(b).toMatchObject({ key: '', value: '' })
  })
})
