import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import { ReviewShipWorkflowVisual } from './ReviewShipWorkflowVisual'
import { AutomationWorkflowVisual } from './ScaleWorkflowVisuals'
import { WorkspacesAnimatedVisual } from './WorkspacesAnimatedVisual'

describe('feature wall workflow action clarity', () => {
  it('shows the review, revision, checks, and publish sequence in order', () => {
    const html = renderToStaticMarkup(<ReviewShipWorkflowVisual />)
    const steps = ['Annotate', 'Revise', 'Run checks', 'Publish']
    const positions = steps.map((step) => html.indexOf(step))

    expect(positions.every((position) => position >= 0)).toBe(true)
    expect(positions).toEqual([...positions].sort((a, b) => a - b))
    expect(html).toContain('Pull / merge request')
  })

  it('renders automations as passive saved workflows with explicit timezones', () => {
    const html = renderToStaticMarkup(<AutomationWorkflowVisual />)

    expect(html).toContain('2 saved workflows')
    expect(html).toContain('Scheduled · Weekdays at 9:00 AM UTC')
    expect(html).toContain('Tomorrow at 9:00 AM UTC')
    expect(html).not.toContain('Run now')
    expect(html).not.toContain('<button')
    expect(html).not.toContain('bg-primary')
  })

  it.each([true, false])(
    'places branch isolation before agent activity when reducedMotion=%s',
    (reducedMotion) => {
      const html = renderToStaticMarkup(<WorkspacesAnimatedVisual reducedMotion={reducedMotion} />)
      const baseBranch = html.indexOf('Base branch')
      const isolatedWorktree = html.indexOf('Isolated worktree + branch')
      const agentActivity = html.indexOf('Agent activity in isolated workspaces')
      const firstWorkspace = html.indexOf('set up orca.yaml')

      expect(html).toContain('.orca/worktrees/orca-yaml')
      expect(html).toContain('feature/orca-yaml')
      expect(baseBranch).toBeGreaterThanOrEqual(0)
      expect(isolatedWorktree).toBeGreaterThan(baseBranch)
      expect(agentActivity).toBeGreaterThan(isolatedWorktree)
      expect(firstWorkspace).toBeGreaterThan(agentActivity)
    }
  )
})
