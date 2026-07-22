import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import { FEATURE_WALL_STEP_IDS } from '../../../../shared/feature-wall-workflows'
import { CliSkillsWorkflowVisual } from './CliSkillsWorkflowVisual'
import { FeatureWallStepVisual } from './FeatureWallStepVisual'

describe('feature wall storyboard truthfulness', () => {
  it.each(FEATURE_WALL_STEP_IDS)('labels the %s visual exactly once as illustrative', (stepId) => {
    const html = renderToStaticMarkup(<FeatureWallStepVisual stepId={stepId} reducedMotion />)
    const labels = html.match(/data-feature-wall-illustrative-example="true"/g) ?? []

    expect(labels).toHaveLength(1)
    expect(html).toContain('Illustrative example')
    expect(html).toContain('data-feature-wall-accessible-summary="true"')
    expect(html).toContain('role="img"')
    expect(html).toMatch(/aria-label="Illustrative example:/)
    expect(html).toContain('animate-none!')
    expect(html).toContain('transition-none!')
  })

  it.each(FEATURE_WALL_STEP_IDS)('keeps the %s visual copy at caption size or larger', (stepId) => {
    const html = renderToStaticMarkup(<FeatureWallStepVisual stepId={stepId} reducedMotion />)

    // Why: these dense storyboards are often viewed inside a scrolled modal;
    // labels below the documented 11px caption size become illegible in captures.
    expect(html).not.toMatch(/text-\[(?:8|8\.5|9|9\.5|10|10\.5)px\]/)
  })

  it('uses the SSH relay CLI once and verifies browser actions with a fresh snapshot', () => {
    const html = renderToStaticMarkup(<CliSkillsWorkflowVisual reducedMotion />)
    const accessibleHtml = renderToStaticMarkup(
      <FeatureWallStepVisual stepId="cli-skills" reducedMotion />
    )
    const snapshots = html.match(/orca snapshot --json/g) ?? []

    expect(html).toContain('Agent terminal · SSH build host')
    expect(html).toContain('orca</span> · SSH host')
    expect(html).toContain('orca skills get orca-cli --full --json')
    expect(html).toContain('result.worktree.displayName = &quot;login-race&quot;')
    expect(html).toContain('result.refs[0] = {&quot;ref&quot;:&quot;@e3&quot;')
    expect(html).toContain('result = {&quot;clicked&quot;:&quot;@e3&quot;}')
    expect(html).toContain('result.refs[0] = {&quot;ref&quot;:&quot;@e4&quot;')
    expect(snapshots).toHaveLength(2)
    expect(html).toContain('Post-click snapshot · status observed')
    expect(html).toContain('Example Orca state')
    expect(html).toContain('data-feature-wall-story-loop="once"')
    expect(html).toContain('data-feature-wall-story-settle-ms="2700"')
    expect(html).not.toContain('orca-dev')
    expect(accessibleHtml).not.toContain('orca-dev')
    expect(accessibleHtml).toContain('through the orca relay')
    expect(html).not.toContain('Live Orca state')
    expect(html).not.toContain('--page app')
    expect(html).not.toContain('>MCP<')
  })

  it('keeps project addition separate from the later setup-backed workspace action', () => {
    const html = renderToStaticMarkup(<FeatureWallStepVisual stepId="add-project" reducedMotion />)

    expect(html).toContain('After the project is added, explicitly create a Git workspace')
    expect(html).toContain('approved shared orca.yaml setup command')
    expect(html).toContain('completed setup output visible')
  })

  it('does not present example agent usage or review data as live', () => {
    const agentHtml = renderToStaticMarkup(<FeatureWallStepVisual stepId="agents" reducedMotion />)
    const reviewHtml = renderToStaticMarkup(
      <FeatureWallStepVisual stepId="review-ship" reducedMotion />
    )

    expect(agentHtml).toContain('Example state')
    expect(agentHtml).not.toContain('>Live<')
    expect(reviewHtml).not.toContain('Illustrative path')
  })
})
