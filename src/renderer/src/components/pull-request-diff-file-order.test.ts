import { describe, expect, it } from 'vitest'
import { orderEntriesByFileTree } from './pull-request-diff-file-order'

describe('orderEntriesByFileTree', () => {
  it('reorders raw API entries into directory-grouped, path-sorted tree DFS order (issue #9485)', () => {
    // Raw GitHub API order mixes directories and a root file arbitrarily.
    const rawOrder = [
      { path: 'src/z.ts' },
      { path: 'readme.md' },
      { path: 'src/a.ts' },
      { path: 'lib/b.ts' }
    ]

    const ordered = orderEntriesByFileTree(rawOrder).map((entry) => entry.path)

    // Tree DFS: directories first (sorted by name) with path-sorted files,
    // then root-level files — lib/, then src/, then readme.md.
    expect(ordered).toEqual(['lib/b.ts', 'src/a.ts', 'src/z.ts', 'readme.md'])
    // The reordering must actually differ from the incoming API order.
    expect(ordered).not.toEqual(rawOrder.map((entry) => entry.path))
  })

  it('preserves entry object identity so section metadata is not lost', () => {
    const first = { path: 'b/one.ts', extra: 1 }
    const second = { path: 'a/two.ts', extra: 2 }
    const ordered = orderEntriesByFileTree([first, second])

    expect(ordered).toEqual([second, first])
    expect(ordered[0]).toBe(second)
    expect(ordered[1]).toBe(first)
  })
})
