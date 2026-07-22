import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import { OrchestrationPage } from './agents-orchestration/OrchestrationPage'
import { StatusesPage } from './agents-orchestration/StatusesPage'
import { TasksAnimatedVisual } from './TasksAnimatedVisual'
import { WorkbenchAnimatedVisual } from './WorkbenchAnimatedVisual'

describe('feature wall reduced-motion visuals', () => {
  it('shows the task-to-workspace outcome immediately', () => {
    const html = renderToStaticMarkup(<TasksAnimatedVisual reducedMotion />)

    expect(html).toContain('Workspace ready')
    expect(html).toContain('Reading issue #')
  })

  it('shows the completed workbench split immediately', () => {
    const html = renderToStaticMarkup(<WorkbenchAnimatedVisual reducedMotion />)

    expect(html).toContain('grid-cols-[1fr_1fr]')
    expect(html).toContain('Claude Code session started')
    expect(html).toContain('review src/auth for missing error handling')
  })

  it('shows both orchestration children without pending entrance states', () => {
    const html = renderToStaticMarkup(<OrchestrationPage active reducedMotion />)

    expect(html).toContain('PR 1/2: migrate users.sql')
    expect(html).toContain('PR 2/2: withSession middleware')
    expect(html).toContain('Writing the users table migration…')
    expect(html).toContain('Sketching withSession middleware…')
    expect(html).not.toContain('data-pending="true"')
  })

  it('uses a finite supported-agent set for the static agents preview', () => {
    const html = renderToStaticMarkup(<StatusesPage active reducedMotion />)
    const supportedAgentPills = html.match(/data-feature-wall-supported-agent-id=/g) ?? []

    expect(supportedAgentPills).toHaveLength(5)
    expect(html).not.toContain('feature-wall-marquee-track')
  })
})
