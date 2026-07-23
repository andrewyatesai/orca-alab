import { beforeAll, describe, expect, it } from 'vitest'
import { FEATURE_WALL_WORKFLOWS } from '../../../../shared/feature-wall-workflows'
import { i18n } from '@/i18n/i18n'
import { getLocalizedFeatureWallWorkflows } from './feature-wall-localized-workflows'

describe('feature wall English catalog alignment', () => {
  beforeAll(async () => {
    await i18n.changeLanguage('en')
  })

  it('serves the current canonical workflow copy instead of stale catalog values', () => {
    // Why: catalog parity checks key presence, not whether an existing English
    // value still matches its source fallback, so product-truth fixes can be masked.
    const localized = getLocalizedFeatureWallWorkflows()

    expect(
      localized.map(({ id, title, meta, lede, steps }) => ({
        id,
        title,
        meta,
        lede,
        steps: steps.map(({ id: stepId, name, title: stepTitle, description }) => ({
          id: stepId,
          name,
          title: stepTitle,
          description
        }))
      }))
    ).toEqual(
      FEATURE_WALL_WORKFLOWS.map(({ id, title, meta, lede, steps }) => ({
        id,
        title,
        meta,
        lede,
        steps: steps.map(({ id: stepId, name, title: stepTitle, description }) => ({
          id: stepId,
          name,
          title: stepTitle,
          description
        }))
      }))
    )
  })
})
