import { readFileSync } from 'node:fs'
import { beforeAll, describe, expect, it } from 'vitest'
import { initGitWasmForTestFromBytes, isGitWasmReady } from './git-line-stats'
import { createQuickOpenIndex, QUICK_OPEN_RESULT_LIMIT } from './quick-open'

// Ported from the deleted TS ranking half of quick-open-search.test.ts: the
// same golden expectations now run THROUGH the Rust orca-text index via wasm.
// (The prepareQuickOpenFiles shape test died with the TS index — preparation
// is internal to the wasm now; the oversized-query no-scan guard is pinned in
// the Rust crate's tests.)

// Captured at import time: the pre-ready contract (empty results, consumers
// recompute via subscribeGitWasmReady).
const preInitResults = createQuickOpenIndex(['src/a.ts']).rank('a')

beforeAll(() => {
  initGitWasmForTestFromBytes(readFileSync(new URL('./orca_git_wasm_bg.wasm', import.meta.url)))
})

describe('quick-open ranking (orca-text wasm)', () => {
  it('ranks empty before the wasm is ready (numstat-style graceful fallback)', () => {
    expect(preInitResults).toEqual([])
    expect(isGitWasmReady()).toBe(true)
  })

  it('returns the first 50 paths with score 0 for an empty query', () => {
    const files = Array.from({ length: 75 }, (_, index) => `src/file-${index}.ts`)

    expect(createQuickOpenIndex(files).rank('')).toEqual(
      files.slice(0, QUICK_OPEN_RESULT_LIMIT).map((path) => ({ path, score: 0 }))
    )
  })

  it('treats a whitespace-only query as empty', () => {
    const index = createQuickOpenIndex(['src/a.ts', 'src/b.ts', 'src/c.ts'])

    expect(index.rank('   ')).toEqual([
      { path: 'src/a.ts', score: 0 },
      { path: 'src/b.ts', score: 0 },
      { path: 'src/c.ts', score: 0 }
    ])
  })

  it('prefers filename substring matches over path-only matches', () => {
    const index = createQuickOpenIndex([
      'button-area/deep/path/file.tsx',
      'src/components/Button.tsx'
    ])

    expect(index.rank('button').map((item) => item.path)).toEqual([
      'src/components/Button.tsx',
      'button-area/deep/path/file.tsx'
    ])
  })

  it('keeps first-seen order for tie-heavy results at the limit boundary', () => {
    const index = createQuickOpenIndex(
      Array.from({ length: 10 }, (_, i) => `src/path-${i}.bin`)
    )

    expect(index.rank('s', 4)).toEqual([
      { path: 'src/path-0.bin', score: 0 },
      { path: 'src/path-1.bin', score: 0 },
      { path: 'src/path-2.bin', score: 0 },
      { path: 'src/path-3.bin', score: 0 }
    ])
  })

  it('returns 50 top-ranked results from a 100k synthetic list', () => {
    const fillerCount = 99_940
    const topCandidateCount = 60
    const index = createQuickOpenIndex([
      ...Array.from(
        { length: fillerCount },
        (_, i) => `n-x-e-x-e-x-d-x-l-x-e/group-${i}/file.ts`
      ),
      ...Array.from({ length: topCandidateCount }, (_, i) => `bulk/special-${i}/needle.ts`)
    ])

    const results = index.rank('needle')

    expect(results).toHaveLength(QUICK_OPEN_RESULT_LIMIT)
    expect(results.map((item) => item.path)).toEqual(
      Array.from({ length: QUICK_OPEN_RESULT_LIMIT }, (_, i) => `bulk/special-${i}/needle.ts`)
    )
  })

  it('returns scores sorted ascending', () => {
    const index = createQuickOpenIndex([
      'src/components/QuickOpen.tsx',
      'quick/open/deep/path/file.tsx',
      'src/q-u-i-c-k-open.ts'
    ])

    const scores = index.rank('quick').map((item) => item.score)

    expect(scores).toEqual([...scores].sort((a, b) => a - b))
  })

  it('returns no results for non-positive limits', () => {
    expect(createQuickOpenIndex(['src/a.ts']).rank('a', 0)).toEqual([])
  })

  it('rejects oversized queries (incl. oversized whitespace before trimming)', () => {
    const index = createQuickOpenIndex(['src/a.ts'])
    expect(index.rank('secret-quick-open'.repeat(2048))).toEqual([])
    expect(index.rank(' '.repeat(2049))).toEqual([])
  })

  it('matches Windows-style path queries against slash-normalized file paths', () => {
    const index = createQuickOpenIndex([
      'src/components/Button.tsx',
      'src/components/ButtonGroup.tsx',
      'src/routes/About.tsx'
    ])

    expect(index.rank('src\\components\\button').map((item) => item.path)).toEqual([
      'src/components/Button.tsx',
      'src/components/ButtonGroup.tsx'
    ])
  })

  it('surfaces exact-path and exact-basename matches (incl. backslash sources)', () => {
    const index = createQuickOpenIndex([
      'src/app.ts',
      'legacy\\provider\\app.ts',
      'other/App.ts',
      'unrelated.ts'
    ])

    expect(index.exactMatches('src/app.ts')).toEqual({ paths: ['src/app.ts'], basenames: [] })
    expect(index.exactMatches('app.ts')).toEqual({
      paths: [],
      basenames: ['src/app.ts', 'legacy\\provider\\app.ts', 'other/App.ts']
    })
  })

  it('reports the prepared file count', () => {
    expect(createQuickOpenIndex(['a.ts', 'b.ts']).fileCount).toBe(2)
  })
})
