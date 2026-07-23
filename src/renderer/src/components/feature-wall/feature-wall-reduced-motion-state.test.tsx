import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import { OrchestrationPage } from './agents-orchestration/OrchestrationPage'
import { StatusesPage } from './agents-orchestration/StatusesPage'
import { BrowserAnimatedVisual } from './BrowserAnimatedVisual'
import { TasksAnimatedVisual } from './TasksAnimatedVisual'
import { WorkbenchAnimatedVisual } from './WorkbenchAnimatedVisual'

describe('feature wall reduced-motion visuals', () => {
  it('shows the task-to-workspace outcome immediately', () => {
    const html = renderToStaticMarkup(<TasksAnimatedVisual reducedMotion />)

    expect(html).toContain('Workspace ready')
    expect(html).toContain('data-feature-wall-task-provider="github"')
    expect(html).toContain('GitHub')
    expect(html).toContain('connected for tasks')
    expect(html).toContain('andrewyatesai/orca-alab')
    expect(html).toContain('Linked issue #1842')
    expect(html).toContain('Reading issue #')
    expect(html).toContain('Open workspace')
    expect(html).not.toContain('Start workspace')
    expect(html.indexOf('connected for tasks')).toBeLessThan(html.indexOf('Workspace ready'))
    expect(html.indexOf('Linked issue #1842')).toBeLessThan(html.indexOf('Workspace ready'))
  })

  it('shows the completed workbench split immediately', () => {
    const html = renderToStaticMarkup(<WorkbenchAnimatedVisual reducedMotion />)

    expect(html).toContain('grid-cols-[1fr_1fr]')
    expect(html).toContain('Claude Code session started')
    expect(html).toContain('review src/auth for missing error handling')
  })

  it('shows the accountable completed orchestration run without pending entrance states', () => {
    const html = renderToStaticMarkup(<OrchestrationPage active reducedMotion />)

    expect(html).toContain('data-feature-wall-orchestration-phase="complete"')
    expect(html).toContain('Task 1 · migrate users.sql')
    expect(html).toContain('Task 2 · withSession (after Task 1)')
    expect(html).toContain('Coordinator accepted the recovered run')
    expect(html).toContain('2 tasks complete · 1 human decision · 1 recovery')
    expect(html).toContain('data-orchestration-ledger="dependency" data-state="released"')
    expect(html).toContain('data-orchestration-ledger="decision" data-state="resolved"')
    expect(html).toContain('data-orchestration-ledger="recovery" data-state="recovered"')
    expect(html.match(/data-agent-state="done"/g)).toHaveLength(3)
    expect(html).not.toContain('data-pending="true"')
  })

  it('shows the verified browser handoff immediately', () => {
    const html = renderToStaticMarkup(<BrowserAnimatedVisual reducedMotion />)

    expect(html).toContain('data-feature-wall-browser-phase="verified"')
    expect(html).toContain('data-feature-wall-browser-context-receipt="sent"')
    expect(html).toContain('Attached')
    expect(html).toContain('DOM')
    expect(html).toContain('Computed styles')
    expect(html).toContain('Source hint · when available')
    expect(html).toContain('Cropped PNG · when available')
    expect(html).toContain('Destination · Claude in this workspace')
    expect(html).toContain(
      'Review before sending—captured context may include visible site content. Profile or cookie reuse is opt-in.'
    )
    expect(html).toContain(
      'Sensitive-context boundary · captured context may include visible site content. Profile or cookie reuse is opt-in.'
    )
    expect(html).toContain('✓ Updated')
    expect(html).toContain('screenshot')
    expect(html).toContain('✓ Verified — Try free still works.')
  })

  it('uses a finite supported-agent set for the static agents preview', () => {
    const html = renderToStaticMarkup(<StatusesPage active reducedMotion />)
    const supportedAgentPills = html.match(/data-feature-wall-supported-agent-id=/g) ?? []

    expect(supportedAgentPills).toHaveLength(5)
    expect(html).not.toContain('feature-wall-marquee-track')
  })
})
