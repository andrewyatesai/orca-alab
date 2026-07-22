import { describe, expect, it } from 'vitest'
import type {
  FeatureWallStepId,
  FeatureWallWorkflowId
} from '../../../../shared/feature-wall-workflows'
import {
  normalizeFeatureWallVisitedSteps,
  normalizeFeatureWallVisitedWorkflows
} from './feature-wall-completion-persistence'
import { getFeatureWallCompletionProgress } from './feature-wall-completion-progress'

function progress(input?: {
  workflows?: FeatureWallWorkflowId[]
  steps?: FeatureWallStepId[]
}): ReturnType<typeof getFeatureWallCompletionProgress> {
  return getFeatureWallCompletionProgress({
    visitedWorkflows: new Set(input?.workflows ?? []),
    visitedSteps: new Set(input?.steps ?? [])
  })
}

describe('getFeatureWallCompletionProgress', () => {
  it('starts with every chapter and step unviewed', () => {
    const result = progress()
    expect(Object.values(result.workflowDone).every((done) => !done)).toBe(true)
    expect(Object.values(result.stepDone).every((done) => !done)).toBe(true)
  })

  it('marks a step viewed without claiming its chapter is complete', () => {
    const result = progress({ workflows: ['start'], steps: ['terminal'] })
    expect(result.stepDone.terminal).toBe(true)
    expect(result.workflowDone.start).toBe(false)
  })

  it('completes a chapter only after every chapter step is viewed', () => {
    const result = progress({
      workflows: ['scale'],
      steps: ['cli-skills', 'orchestration', 'automations']
    })
    expect(result.workflowDone.scale).toBe(true)
    expect(result.workflowDone.anywhere).toBe(false)
  })

  it('completes the single-step ship chapter after its review screen is viewed', () => {
    const result = progress({ workflows: ['ship'], steps: ['review-ship'] })
    expect(result.workflowDone.ship).toBe(true)
  })
})

describe('feature wall completion persistence normalization', () => {
  it('keeps lifecycle chapters and drops stale or duplicate ids', () => {
    expect(
      normalizeFeatureWallVisitedWorkflows(['start', 'plan', 'plan', 'workbench', 'bogus'])
    ).toEqual(['start', 'plan'])
  })

  it('keeps catalog steps and drops stale or duplicate ids', () => {
    expect(
      normalizeFeatureWallVisitedSteps([
        'terminal',
        'automations',
        'computer-use',
        'automations',
        'pr-view',
        'bogus'
      ])
    ).toEqual(['terminal', 'automations', 'computer-use'])
  })
})
