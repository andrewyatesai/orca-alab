import { describe, expect, it, vi } from 'vitest'

vi.mock('@/i18n/i18n', () => ({
  translate: (_key: string, fallback: string) => `localized:${fallback}`
}))

vi.mock('@/i18n/localized-catalog', () => ({
  createLocalizedCatalog:
    <T>(builder: () => T) =>
    () =>
      builder()
}))

import { getLocalizedFeatureWallWorkflows } from './feature-wall-localized-workflows'

describe('localized feature wall workflows', () => {
  it('localizes every displayed workflow and step field while preserving navigation data', () => {
    const workflows = getLocalizedFeatureWallWorkflows()
    const steps = workflows.flatMap((workflow) => workflow.steps)

    expect(workflows).toHaveLength(6)
    expect(steps).toHaveLength(11)
    for (const workflow of workflows) {
      expect(workflow.title).toMatch(/^localized:/)
      expect(workflow.meta).toMatch(/^localized:/)
      expect(workflow.lede).toMatch(/^localized:/)
      expect(workflow.docsUrl).toMatch(/^https:\/\/www\.onorca\.dev\/docs\//)
    }
    for (const step of steps) {
      expect(step.name).toMatch(/^localized:/)
      expect(step.title).toMatch(/^localized:/)
      expect(step.description).toMatch(/^localized:/)
    }
    expect(steps.find((step) => step.id === 'computer-use')?.availabilityLabel).toBe(
      'localized:Beta'
    )
    expect(steps.find((step) => step.id === 'remote-mobile')?.availabilityLabel).toBe(
      'localized:Mobile beta'
    )
  })
})
