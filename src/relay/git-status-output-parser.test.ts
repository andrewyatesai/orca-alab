import { describe, expect, it } from 'vitest'
import { parseStatusOutput } from './git-status-output-parser'

describe('parseStatusOutput', () => {
  it('marks staged S... submodule rows as commit-changed gitlinks', () => {
    const parsed = parseStatusOutput(
      '1 M. S... 160000 160000 160000 aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb flutter_mine\n'
    )

    expect(parsed.entries).toEqual([
      {
        path: 'flutter_mine',
        status: 'modified',
        area: 'staged',
        submodule: { commitChanged: true, trackedChanges: false, untrackedChanges: false }
      }
    ])
  })

  it('parses branch headers, ahead/behind, and mixed entry kinds', () => {
    const parsed = parseStatusOutput(
      [
        '# branch.oid abc123',
        '# branch.head feature',
        '# branch.upstream origin/feature',
        '# branch.ab +2 -1',
        '1 M. N... 100644 100644 100644 aaa bbb src/staged.ts',
        '1 .M N... 100644 100644 100644 aaa bbb src/unstaged.ts',
        '2 R. N... 100644 100644 100644 aaa bbb R100 new name.ts\told name.ts',
        '? untracked.ts',
        '! ignored.ts',
        ''
      ].join('\n')
    )

    expect(parsed.head).toBe('abc123')
    expect(parsed.branch).toBe('refs/heads/feature')
    expect(parsed.upstreamStatus).toEqual({
      hasUpstream: true,
      upstreamName: 'origin/feature',
      ahead: 2,
      behind: 1
    })
    expect(parsed.ignoredPaths).toEqual(['ignored.ts'])
    expect(parsed.entries).toEqual([
      { path: 'src/staged.ts', status: 'modified', area: 'staged' },
      { path: 'src/unstaged.ts', status: 'modified', area: 'unstaged' },
      { path: 'new name.ts', status: 'renamed', area: 'staged', oldPath: 'old name.ts' },
      { path: 'untracked.ts', status: 'untracked', area: 'untracked' }
    ])
  })

  it('reports no upstream when branch.upstream is absent', () => {
    const parsed = parseStatusOutput('# branch.head main\n')
    expect(parsed.branch).toBe('refs/heads/main')
    expect(parsed.upstreamStatus).toEqual({ hasUpstream: false, ahead: 0, behind: 0 })
  })
})
