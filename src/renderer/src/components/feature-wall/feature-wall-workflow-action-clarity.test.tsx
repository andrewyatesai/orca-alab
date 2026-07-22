import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import { ReviewShipWorkflowVisual } from './ReviewShipWorkflowVisual'
import { AutomationWorkflowVisual } from './ScaleWorkflowVisuals'
import { WorkspacesAnimatedVisual } from './WorkspacesAnimatedVisual'

describe('feature wall workflow action clarity', () => {
  it('shows the review, revision, checks, and publish sequence in order', () => {
    const html = renderToStaticMarkup(<ReviewShipWorkflowVisual reducedMotion />)
    const steps = [
      'Compare candidates',
      'Annotate + send revision',
      'Review decision',
      'Failed check / conflict',
      'Return, resolve + retry',
      'Checks pass',
      'Confirm commit + push, then PR / MR',
      'Workspace archived'
    ]
    const positions = steps.map((step, index) =>
      index === steps.length - 1 ? html.lastIndexOf(step) : html.indexOf(step)
    )

    expect(positions.every((position) => position >= 0)).toBe(true)
    expect(positions).toEqual([...positions].sort((a, b) => a - b))
  })

  it('renders automations as passive saved workflows with explicit timezones', () => {
    const html = renderToStaticMarkup(<AutomationWorkflowVisual reducedMotion />)

    expect(html).toContain('2 saved workflows')
    expect(html).toContain('Scheduled · Weekdays · Precheck enabled')
    expect(html).toContain('Recovered on rerun')
    expect(html).toContain('Fresh workspace · history and output retained')
    expect(html).toContain('Previous attempt · Precheck failed')
    expect(html).toContain('Target unavailable · run retained')
    expect(html).toContain('Tomorrow at 9:00 AM UTC')
    expect(html).not.toContain('Run now')
    expect(html).not.toContain('<button')
    expect(html).not.toContain('bg-primary')
  })

  it.each([true, false])(
    'places branch isolation before agent activity when reducedMotion=%s',
    (reducedMotion) => {
      const html = renderToStaticMarkup(<WorkspacesAnimatedVisual reducedMotion={reducedMotion} />)
      const baseBranch = html.indexOf('Git project · base branch')
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
