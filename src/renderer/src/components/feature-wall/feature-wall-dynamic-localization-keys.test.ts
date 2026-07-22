import { describe, expect, it } from 'vitest'
import en from '@/i18n/locales/en.json'
import es from '@/i18n/locales/es.json'
import ja from '@/i18n/locales/ja.json'
import ko from '@/i18n/locales/ko.json'
import zh from '@/i18n/locales/zh.json'

const BROWSER_KEYS = ['a140000001', 'a150000001', 'a150000002', 'a150000003'] as const
const REVIEW_KEYS = [
  'header.title',
  'header.detail',
  'timeline.title',
  'footer.disclaimer',
  'candidate.title',
  'candidate.a',
  'candidate.aFiles',
  'candidate.b',
  'candidate.bFiles',
  'candidate.focused',
  'panel.progress',
  'evidence.archive',
  'evidence.blocker',
  'evidence.blockerDetail',
  'evidence.commit',
  'evidence.compare',
  'evidence.decision',
  'evidence.note',
  'evidence.passed',
  'evidence.rereview',
  'evidence.resolve',
  'evidence.retry',
  'evidence.review',
  'evidence.send',
  'evidence.stage',
  'story.annotate.detail',
  'story.annotate.title',
  'story.archive.detail',
  'story.archive.title',
  'story.blocked.detail',
  'story.blocked.title',
  'story.compare.detail',
  'story.compare.title',
  'story.confirm.detail',
  'story.confirm.title',
  'story.decision.detail',
  'story.decision.title',
  'story.passed.detail',
  'story.passed.title',
  'story.rereview.detail',
  'story.rereview.title',
  'story.retry.detail',
  'story.retry.title',
  'story.stage.detail',
  'story.stage.title'
] as const

const CATALOGS = { en, es, ja, ko, zh } as const

function readPath(value: unknown, path: string): unknown {
  return path.split('.').reduce<unknown>((current, segment) => {
    if (!current || typeof current !== 'object') {
      return undefined
    }
    return (current as Record<string, unknown>)[segment]
  }, value)
}

function placeholders(value: string): string[] {
  return [...value.matchAll(/{{[^}]+}}/g)].map((match) => match[0]).sort()
}

describe('feature wall dynamic localization keys', () => {
  it('keeps every computed walkthrough key present and placeholder-compatible', () => {
    // Why: the catalog scanner sees literal translate calls only; these two
    // storyboard registries intentionally select their keys at runtime.
    const paths = [
      ...BROWSER_KEYS.map(
        (key) => `auto.components.feature.wall.BrowserDesignPayloadSummary.${key}`
      ),
      ...REVIEW_KEYS.map((key) => `auto.components.feature.wall.ReviewShipWorkflowVisual.${key}`)
    ]

    expect(paths).toHaveLength(49)
    for (const path of paths) {
      const english = readPath(en, path)
      expect(english, `missing en:${path}`).toEqual(expect.any(String))
      for (const [locale, catalog] of Object.entries(CATALOGS)) {
        const localized = readPath(catalog, path)
        expect(localized, `missing ${locale}:${path}`).toEqual(expect.any(String))
        expect(placeholders(String(localized)), `placeholder mismatch ${locale}:${path}`).toEqual(
          placeholders(String(english))
        )
      }
    }
  })
})
