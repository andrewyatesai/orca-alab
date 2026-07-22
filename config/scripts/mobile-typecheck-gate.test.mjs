import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const rootPackageJson = JSON.parse(
  readFileSync(fileURLToPath(new URL('../../package.json', import.meta.url)), 'utf8')
)

// Why: mobile/ is outside every root tsconfig and outside the type-aware
// lint paths, so its never-guarded classifiers (e.g. github-check-summary)
// are only compile-checked if the root typecheck gate reaches into mobile.
describe('mobile typecheck gate', () => {
  it('root typecheck script runs the mobile typecheck', () => {
    expect(rootPackageJson.scripts.typecheck).toContain('pnpm run typecheck:mobile')
  })

  it('typecheck:mobile delegates to the mobile package', () => {
    expect(rootPackageJson.scripts['typecheck:mobile']).toBe('pnpm --dir mobile typecheck')
  })
})
