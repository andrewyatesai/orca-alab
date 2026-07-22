import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import { ComputerUseWorkflowVisual, RemoteMobileWorkflowVisual } from './AnywhereWorkflowVisuals'
import {
  AddProjectWorkflowVisual,
  TerminalFirstWorkflowVisual
} from './TerminalProjectWorkflowVisuals'

describe('feature wall host-model visuals', () => {
  it('keeps the terminal preview scoped to the active workspace', () => {
    const html = renderToStaticMarkup(<TerminalFirstWorkflowVisual />)

    expect(html).toContain('Persistent shell session ready')
    expect(html).toContain('Ready in the active workspace terminal.')
    expect(html).toContain('Sessions survive reconnects')
    expect(html).toContain('aterm · Rust')
    expect(html).not.toContain('Last login')
    expect(html).not.toContain('>$</span>')
    expect(html).not.toContain('before you choose a project')
  })

  it('separates project location from codebase method', () => {
    const html = renderToStaticMarkup(<AddProjectWorkflowVisual />)

    expect(html).toContain('1 · Location')
    expect(html).toContain('This computer')
    expect(html).toContain('SSH host')
    expect(html).toContain('Paired Orca runtime')
    expect(html).toContain('2 · Codebase')
    expect(html).toContain('Open existing folder')
    expect(html).toContain('Clone repository')
    expect(html).toContain('Create project')
    expect(html).not.toContain('lucide-chevron-right')
  })

  it('connects the full desktop client and mobile companion through the runtime', () => {
    const html = renderToStaticMarkup(<RemoteMobileWorkflowVisual />)

    expect(html).toContain('Full client')
    expect(html).toContain('Companion')
    expect(html).toContain('Orca desktop')
    expect(html).toContain('Orca Mobile')
    expect(html).toContain('Orca runtime')
    expect(html).toContain('data-feature-wall-mobile-beta="true"')
    expect(html).toContain('Beta')
    expect(html).toContain('Desktop source of truth')
    expect(html).toContain('Optional downstream')
    expect(html).toContain('SSH project host')
    expect(html).not.toContain('SSH / remote')
  })

  it('shows Computer Use boundaries without a duplicate platform badge', () => {
    const html = renderToStaticMarkup(<ComputerUseWorkflowVisual />)

    expect(html).toContain('macOS permissions')
    expect(html).toContain('Accessibility + Screen Recording')
    expect(html).toContain('Visible app scope')
    expect(html).toContain('Advertised actions only')
    expect(html).not.toContain('macOS only')
  })
})
