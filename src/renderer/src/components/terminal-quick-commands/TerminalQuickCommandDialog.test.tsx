// @vitest-environment happy-dom

import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import type { TerminalQuickCommand } from '../../../../shared/types'
import { initGitWasmForTestFromBytes } from '@/lib/git-wasm/git-line-stats'
import { TerminalQuickCommandDialog } from './TerminalQuickCommandDialog'

// The dialog classifies/validates commands through the git wasm. happy-dom makes
// import.meta.url a non-file URL, so read the bytes via __dirname.
beforeAll(() => {
  initGitWasmForTestFromBytes(
    readFileSync(join(__dirname, '../../lib/git-wasm/orca_git_wasm_bg.wasm'))
  )
})

const mountedRoots: Root[] = []

async function renderDialog(command: TerminalQuickCommand): Promise<void> {
  const container = document.createElement('div')
  document.body.appendChild(container)
  const root = createRoot(container)
  mountedRoots.push(root)

  await act(async () => {
    root.render(
      <TerminalQuickCommandDialog
        open={true}
        mode="add"
        command={command}
        repos={[]}
        onOpenChange={vi.fn()}
        onSave={vi.fn()}
      />
    )
  })
}

function findAnimatedRowContaining(text: string): HTMLElement {
  const row = Array.from(document.body.querySelectorAll<HTMLElement>('[aria-hidden]')).find(
    (element) => element.textContent?.includes(text)
  )
  if (!row) {
    throw new Error(`Could not find animated row containing ${text}`)
  }
  return row
}

describe('TerminalQuickCommandDialog animation structure', () => {
  afterEach(async () => {
    await act(async () => {
      for (const root of mountedRoots.splice(0)) {
        root.unmount()
      }
    })
    document.body.innerHTML = ''
  })

  it('keeps agent-only fields mounted as collapsed animated rows in terminal mode', async () => {
    await renderDialog({
      id: 'qc-1',
      label: 'Start dev server',
      action: 'terminal-command',
      command: 'npm run dev',
      appendEnter: true,
      scope: { type: 'global' }
    })

    const agentRow = findAnimatedRowContaining('Agent')
    const promptHelpRow = findAnimatedRowContaining('Supports skills')

    expect(agentRow.getAttribute('aria-hidden')).toBe('true')
    expect(agentRow.className).toContain('transition-[grid-template-rows]')
    expect(agentRow.className).toContain('grid-rows-[0fr]')
    expect(promptHelpRow.getAttribute('aria-hidden')).toBe('true')
    expect(promptHelpRow.className).toContain('grid-rows-[0fr]')
  })
})
