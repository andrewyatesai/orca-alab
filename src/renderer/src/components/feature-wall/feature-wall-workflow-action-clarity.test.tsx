import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import { AgentAttentionWorkflowVisual } from './AgentAttentionWorkflowVisual'
import { BrowserDesignPayloadSummary } from './BrowserDesignPayloadSummary'
import { ReviewShipWorkflowVisual } from './ReviewShipWorkflowVisual'
import { AutomationWorkflowVisual } from './ScaleWorkflowVisuals'
import { TerminalFirstWorkflowVisual } from './TerminalProjectWorkflowVisuals'
import { WorkbenchContextWorkflowVisual } from './WorkbenchContextWorkflowVisual'
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
      'Re-review resolved diff',
      'Stage focused hunk',
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
      const firstWorkspace = html.indexOf('set up orca.yaml', agentActivity)

      expect(html).toContain('orca-yaml-codex')
      expect(html).not.toContain('.orca/worktrees')
      expect(html).toContain('feature/orca-yaml')
      expect(baseBranch).toBeGreaterThanOrEqual(0)
      expect(isolatedWorktree).toBeGreaterThan(baseBranch)
      expect(agentActivity).toBeGreaterThan(isolatedWorktree)
      expect(firstWorkspace).toBeGreaterThan(agentActivity)
    }
  )

  it('keeps board planning separate from the Git worktree race', () => {
    const initialHtml = renderToStaticMarkup(<WorkspacesAnimatedVisual reducedMotion={false} />)
    const html = renderToStaticMarkup(<WorkspacesAnimatedVisual reducedMotion />)

    expect(initialHtml).toContain('data-board-move-state="ready"')
    expect(html).toContain('data-feature-wall-workspace-board="status-lanes"')
    expect(html).toContain('Workspace board')
    expect(html).toContain('Existing workspaces · user-assigned status')
    expect(html).toContain('Todo')
    expect(html).toContain('In progress')
    expect(html).toContain('In review')
    expect(html).toContain('Done')
    expect(html).toContain('data-board-move-owner="user"')
    expect(html).toContain('data-board-move-state="complete"')
    expect(html.indexOf('Workspace board')).toBeLessThan(
      html.indexOf('Agent activity in isolated workspaces')
    )
  })

  it('settles once on a user-selected winner with visible review rationale', () => {
    const html = renderToStaticMarkup(<WorkspacesAnimatedVisual reducedMotion />)

    expect(html).toContain('data-feature-wall-story-loop="once"')
    expect(html).toContain('data-feature-wall-story-settle-ms="3500"')
    expect(html).toContain('data-winner-selection="user"')
    expect(html).toContain('data-winner-rationale="checks-and-focused-diff"')
    expect(html).toContain('Human choice · all checks passed; smallest focused diff')
    expect(html).toContain('Codex · 18/18 · +42 −8')
  })

  it('shows AI Vault discovery and its resume, jump, and log boundaries', () => {
    const html = renderToStaticMarkup(<AgentAttentionWorkflowVisual reducedMotion />)

    expect(html).toContain('data-feature-wall-ai-vault="session-history"')
    expect(html).toContain('Agent Session History')
    expect(html).not.toContain('Optional · experimental')
    expect(html).toContain('Workspace scope · reconnect')
    expect(html).toContain('Jump to owned worktree')
    expect(html).toContain('Resume · content + compatible target')
    expect(html).toContain('View log · local path available')
  })

  it('shows the default-on Floating Workspace and its local ownership boundary', () => {
    const html = renderToStaticMarkup(<WorkbenchContextWorkflowVisual reducedMotion />)

    expect(html).toContain('data-feature-wall-floating-workspace="local-scratchpad"')
    expect(html).toContain('Floating Workspace')
    expect(html).toContain('Default on')
    expect(html).toContain('Chosen local directory · stays local during SSH/runtime focus')
    expect(html).toContain('Cross-repo agent')
    expect(html).toContain('Scratch terminal')
    expect(html).toContain('Markdown note')
    expect(html).toContain('Browser tab')
  })

  it('shows Quick Command trust before project commands can run', () => {
    const html = renderToStaticMarkup(<TerminalFirstWorkflowVisual />)

    expect(html).toContain('data-feature-wall-quick-command-trust="project-review"')
    expect(html).toContain('Project orca.yaml · inert until review; changes require re-review')
    expect(html).toContain('User-owned commands · run normally')
  })

  it('marks optional browser payload fields and authenticated reuse as opt-in', () => {
    const html = renderToStaticMarkup(<BrowserDesignPayloadSummary mode="sent" />)

    expect(html).toContain('DOM')
    expect(html).toContain('Computed styles')
    expect(html).toContain('Source hint · when available')
    expect(html).toContain('Cropped PNG · when available')
    expect(html).toContain('Profile or cookie reuse is opt-in')
  })
})
