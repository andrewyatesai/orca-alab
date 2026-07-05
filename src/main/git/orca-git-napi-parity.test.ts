import { StringDecoder } from 'node:string_decoder'
import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { loadRustGitBinding } from '../daemon/rust-git-addon'
import { StatusPorcelainParser } from './status-porcelain-parser'
import { parseWorktreeListTs } from './worktree'
import { parseGitHistoryLog } from '../../shared/git-history-log-parser'
import { parseNumstat } from '../../shared/git-uncommitted-line-stats'
import { decodeGitCQuotedPath } from '../../shared/git-cquoted-path'
import { assertGitPushTargetShape } from '../../shared/git-push-target-validation'
import { isBinaryBuffer } from '../../shared/binary-buffer'

// Dual-run parity proof: the verified Rust `orca-git` parsers (exposed via the
// napi addon) must be a faithful drop-in for the live TS parsers. Each fixture
// is parsed by BOTH and asserted deepEqual. This is the load-bearing evidence
// for a later cut-over; it touches none of the live git-status paths.
//
// Skips cleanly when the .node is absent (CI without a native build), so the
// suite still passes there.

const binding = loadRustGitBinding()
const suite = binding ? describe : describe.skip
// Safe under describe.skip: the closure never runs when the binding is null.
const git = binding!

/** Flatten a live TS `StatusPorcelainParser` into the exact JSON shape the Rust
 *  `status_parse_result_to_json` builder emits (None/undefined fields omitted,
 *  branch flattened to the top level, entries sliced to the cap when stopped). */
function tsStatusShape(parser: StatusPorcelainParser, stopped: boolean, limit: number): unknown {
  const count = parser.statusLength
  const entries = stopped ? parser.entries.slice(0, Math.min(count, limit)) : parser.entries
  const out: Record<string, unknown> = {
    entries,
    ignoredPaths: parser.ignoredPaths,
    unmergedLines: parser.unmergedLines
  }
  const branch = parser.branch
  if (branch.head !== undefined) {
    out.head = branch.head
  }
  if (branch.branch !== undefined) {
    out.branch = branch.branch
  }
  if (branch.upstreamName !== undefined) {
    out.upstreamName = branch.upstreamName
  }
  if (branch.upstreamAheadBehind) {
    out.ahead = branch.upstreamAheadBehind.ahead
    out.behind = branch.upstreamAheadBehind.behind
  }
  if (stopped) {
    out.didHitLimit = true
  }
  out.statusLength = count
  return out
}

/** Live TS one-shot status parse — mirrors `parse_status_porcelain`. The raw
 *  bytes are lossy-utf8-decoded the same way the daemon decodes git stdout. */
function tsStatusOneShot(bytes: Buffer, limit: number): unknown {
  const parser = new StatusPorcelainParser()
  const stopped = parser.update(bytes.toString('utf8'), limit)
  if (!stopped) {
    parser.finish()
  }
  return tsStatusShape(parser, stopped, limit)
}

/** napi streaming parse: feed RAW byte chunks (the Rust parser carries bytes). */
function napiStatusStreaming(bytes: Buffer, limit: number, chunkSize: number): unknown {
  const parser = new git.GitStatusParser()
  let stopped = false
  for (let i = 0; i < bytes.length && !stopped; i += chunkSize) {
    stopped = parser.update(bytes.subarray(i, i + chunkSize), limit)
  }
  if (!stopped) {
    parser.finish()
  }
  return JSON.parse(parser.result(limit))
}

/** Live TS streaming parse: decode chunks through a StringDecoder exactly as
 *  the daemon runner does (carrying incomplete utf8 sequences across chunks). */
function tsStatusStreaming(bytes: Buffer, limit: number, chunkSize: number): unknown {
  const parser = new StatusPorcelainParser()
  const decoder = new StringDecoder('utf8')
  let stopped = false
  for (let i = 0; i < bytes.length && !stopped; i += chunkSize) {
    const decoded = decoder.write(bytes.subarray(i, i + chunkSize))
    if (decoded) {
      stopped = parser.update(decoded, limit)
    }
  }
  if (!stopped) {
    const tail = decoder.end()
    if (tail) {
      stopped = parser.update(tail, limit)
    }
  }
  if (!stopped) {
    parser.finish()
  }
  return tsStatusShape(parser, stopped, limit)
}

/** Convert the live TS numstat Map into the Rust `numstat_to_json` shape (a
 *  plain object keyed by path; binary files become `{}`). */
function tsNumstatShape(bytes: Buffer): Record<string, unknown> {
  const out: Record<string, unknown> = {}
  for (const [path, stats] of parseNumstat(bytes.toString('utf8'))) {
    const inner: Record<string, number> = {}
    if (stats.added !== undefined) {
      inner.added = stats.added
    }
    if (stats.removed !== undefined) {
      inner.removed = stats.removed
    }
    out[path] = inner
  }
  return out
}

/** Live TS untracked-additions counter — transcribed from the inline logic in
 *  `git-uncommitted-line-stats.ts`'s `countFileAdditions` (binary → null, empty
 *  → 0, else trailing-newline-aware line count). */
function tsCountAdditions(bytes: Buffer): number | null {
  if (isBinaryBuffer(bytes)) {
    return null
  }
  if (bytes.length === 0) {
    return 0
  }
  let newlines = 0
  for (let i = 0; i < bytes.length; i += 1) {
    if (bytes[i] === 0x0a) {
      newlines += 1
    }
  }
  return bytes.at(-1) === 0x0a ? newlines : newlines + 1
}

/** Live TS line-stats — transcribed 1:1 from `diff-line-stats.ts`'s
 *  `computeLineStats` (it lives in the renderer tsconfig project, which the node
 *  project cannot import; `line_count.rs` is the Rust port of the same algorithm).
 *  forEachDiffLine splits on `\n`, so `.split('\n')` is the faithful equivalent. */
function tsComputeLineStats(
  original: string,
  modified: string,
  status: string
): { added: number; removed: number } | null {
  if (original.length + modified.length > 500_000) {
    return null
  }
  const lineCount = (content: string): number => content.split('\n').length
  if (status === 'added') {
    return { added: modified ? lineCount(modified) : 0, removed: 0 }
  }
  if (status === 'deleted') {
    return { added: 0, removed: original ? lineCount(original) : 0 }
  }
  const origCounts = new Map<string, number>()
  let originalLineCount = 0
  for (const line of original.split('\n')) {
    originalLineCount += 1
    origCounts.set(line, (origCounts.get(line) ?? 0) + 1)
  }
  let modifiedLineCount = 0
  let matched = 0
  for (const line of modified.split('\n')) {
    modifiedLineCount += 1
    const count = origCounts.get(line) ?? 0
    if (count > 0) {
      origCounts.set(line, count - 1)
      matched += 1
    }
  }
  return { added: modifiedLineCount - matched, removed: originalLineCount - matched }
}

// "? \xE9.txt\n": a lone 0xE9 is invalid UTF-8 and must become U+FFFD lossily.
const invalidUtf8Path = Buffer.from([0x3f, 0x20, 0xe9, 0x2e, 0x74, 0x78, 0x74, 0x0a])

const statusFixtures: { name: string; bytes: Buffer; limit: number }[] = [
  {
    name: 'branch headers + type-1 staged/unstaged + untracked + ignored',
    bytes: Buffer.from(
      '# branch.oid abc123\n# branch.head feature/x\n# branch.upstream origin/feature/x\n' +
        '# branch.ab +2 -1\n' +
        '1 M. N... 100644 100644 100644 aaaa aaaa src/staged.ts\n' +
        '1 .M N... 100644 100644 100644 bbbb bbbb src/unstaged.ts\n' +
        '? new.txt\n! dist/\n'
    ),
    limit: 0
  },
  {
    name: 'type-2 rename with old path',
    bytes: Buffer.from('2 R. N... 100644 100644 100644 aaaa bbbb R100 new.ts\told.ts\n'),
    limit: 0
  },
  {
    name: 'type-2 rename with spaces in both paths',
    bytes: Buffer.from('2 R. N... 1 1 1 a a R100 new name.ts\told name.ts\n'),
    limit: 0
  },
  {
    name: 'unmerged record collected as raw line',
    bytes: Buffer.from('u UU N... 100644 100644 100644 100644 aa bb cc both.ts\n'),
    limit: 0
  },
  {
    name: 'detached head clears branch',
    bytes: Buffer.from('# branch.oid def\n# branch.head (detached)\n? a.txt\n'),
    limit: 0
  },
  {
    name: 'submodule dirtiness flags',
    bytes: Buffer.from('1 AM S..U 000000 160000 160000 0000 7844 nested-repo\n'),
    limit: 0
  },
  { name: 'CRLF output', bytes: Buffer.from('? win.txt\r\n? second.txt\r\n'), limit: 0 },
  {
    name: 'C-quoted octal path (UTF-8 é)',
    bytes: Buffer.from('? "\\303\\251.txt"\n'),
    limit: 0
  },
  {
    name: 'C-quoted tab escape in changed path',
    bytes: Buffer.from('1 .M N... 1 1 1 a a "tab\\tname.ts"\n'),
    limit: 0
  },
  { name: 'invalid UTF-8 path byte (lossy U+FFFD)', bytes: invalidUtf8Path, limit: 0 },
  { name: 'empty input', bytes: Buffer.from(''), limit: 0 },
  {
    name: 'limit cap trips one past the limit',
    bytes: Buffer.from('? f0.txt\n? f1.txt\n? f2.txt\n? f3.txt\n? f4.txt\n'),
    limit: 3
  }
]

const streamingFixtures: { name: string; bytes: Buffer; limit: number; chunkSize: number }[] = [
  {
    name: 'chunk-carry across a partial untracked line',
    bytes: Buffer.from('? partial-name.txt\n'),
    limit: 0,
    chunkSize: 5
  },
  {
    name: 'multi-record split across small chunks',
    bytes: Buffer.from('# branch.oid abc\n1 M. N... 1 1 1 a a a.ts\n? b.txt\n'),
    limit: 0,
    chunkSize: 7
  },
  {
    name: 'limit cap reached mid-stream',
    bytes: Buffer.from('? a.txt\n? b.txt\n? c.txt\n? d.txt\n? e.txt\n'),
    limit: 2,
    chunkSize: 3
  }
]

const numstatFixtures: { name: string; bytes: Buffer }[] = [
  {
    name: 'text added/removed counts',
    bytes: Buffer.from('3\t4\tsrc/app.ts\n10\t0\tsrc/new.ts\n')
  },
  { name: 'binary dash columns → empty object', bytes: Buffer.from('-\t-\tassets/logo.png\n') },
  {
    name: 'text rename brace + arrow normalization',
    bytes: Buffer.from('2\t1\tsrc/{old => new}/file.ts\n2\t1\told.ts => new.ts\n')
  },
  { name: '-z NUL-delimited rename', bytes: Buffer.from('2\t1\t\0old.ts\0new.ts\0') },
  {
    name: '-z literal arrow filename kept verbatim',
    bytes: Buffer.from('1\t0\tdocs/a => b.txt\0')
  },
  { name: 'C-quoted tab path key', bytes: Buffer.from('1\t1\t"tab\\tfile.txt"\n') },
  { name: 'empty input', bytes: Buffer.from('') }
]

const worktreeFixtures: { name: string; output: string; nulDelimited: boolean }[] = [
  {
    name: 'main + linked + bare blocks',
    output:
      '\nworktree /repo\nHEAD abc123\nbranch refs/heads/main\n\n' +
      'worktree /repo-feature\nHEAD def456\nbranch refs/heads/feature/test\n\n' +
      'worktree /repo-bare\nHEAD 0000000\nbare\n',
    nulDelimited: false
  },
  {
    name: 'detached head has no branch',
    output: 'worktree /d\nHEAD abc123\ndetached\n',
    nulDelimited: false
  },
  {
    name: 'sparse flag',
    output: 'worktree /repo\nHEAD abc\nbranch refs/heads/main\nsparse\n',
    nulDelimited: false
  },
  {
    name: 'path with spaces',
    output: 'worktree /path/to/my worktree\nHEAD ccc\nbranch refs/heads/main\n',
    nulDelimited: false
  },
  {
    name: 'CRLF blocks',
    output: 'worktree /a\r\nHEAD aaa\r\nbranch refs/heads/main\r\n',
    nulDelimited: false
  },
  { name: 'empty input', output: '   \n\n  ', nulDelimited: false },
  {
    name: '-z NUL form with newline in a linked path',
    output: [
      'worktree /repo',
      'HEAD abc',
      'branch refs/heads/main',
      '',
      'worktree /repo/lin\nked',
      'HEAD def',
      'branch refs/heads/nl',
      ''
    ].join('\0'),
    nulDelimited: true
  }
]

const decodeFixtures = [
  'plain/path.ts',
  '"quoted/path.ts"',
  '"tab\\tfile.txt"',
  '"new\\nline.txt"',
  '"\\303\\251.txt"',
  '"a\\142c.txt"',
  '"\\"quoted\\".txt"',
  'old.ts => new.ts'
]

const countFixtures: { name: string; bytes: Buffer }[] = [
  { name: 'trailing newline', bytes: Buffer.from('a\nb\nc\n') },
  { name: 'no trailing newline', bytes: Buffer.from('a\nb\nc') },
  { name: 'empty', bytes: Buffer.from('') },
  { name: 'binary (NUL)', bytes: Buffer.from([0x00, 0x01, 0x02]) },
  { name: 'single line', bytes: Buffer.from('one line') },
  { name: 'newlines only', bytes: Buffer.from('\n\n\n') }
]

const lineStatsFixtures: { name: string; original: string; modified: string; status: string }[] = [
  { name: 'added', original: '', modified: 'a\nb', status: 'added' },
  { name: 'deleted', original: 'a\nb\n', modified: '', status: 'deleted' },
  { name: 'modified multiset', original: 'a\nb\nc', modified: 'a\nc\nd', status: 'modified' },
  {
    name: 'modified with repeats',
    original: 'same\nold\nkept',
    modified: 'same\nnew\nkept',
    status: 'modified'
  },
  // Large-input guard: both return null (ASCII keeps byte length == UTF-16 length).
  {
    name: 'large guard',
    original: 'x'.repeat(250_001),
    modified: 'y'.repeat(250_000),
    status: 'modified'
  }
]

suite('orca-git napi ↔ TS parser parity', () => {
  it('exposes the orca-git engine marker', () => {
    expect(git.gitEngine()).toBe('orca-git')
  })

  describe('status porcelain one-shot', () => {
    for (const fixture of statusFixtures) {
      it(fixture.name, () => {
        const napi = JSON.parse(git.parseStatusPorcelain(fixture.bytes, fixture.limit))
        const ts = tsStatusOneShot(fixture.bytes, fixture.limit)
        expect(napi).toEqual(ts)
      })
    }
  })

  describe('status porcelain streaming (chunked) matches TS and one-shot', () => {
    for (const fixture of streamingFixtures) {
      it(fixture.name, () => {
        const napi = napiStatusStreaming(fixture.bytes, fixture.limit, fixture.chunkSize)
        const ts = tsStatusStreaming(fixture.bytes, fixture.limit, fixture.chunkSize)
        expect(napi).toEqual(ts)
        // Streaming must agree with the one-shot scan for the same bytes + cap.
        expect(napi).toEqual(JSON.parse(git.parseStatusPorcelain(fixture.bytes, fixture.limit)))
      })
    }
  })

  it('relay regression: cap stops the scan instead of materializing all rows', () => {
    const total = 150_000
    const limit = 10_000
    const lines: string[] = []
    for (let i = 0; i < total; i += 1) {
      lines.push(`1 M. N... 100644 100644 100644 aaaa bbbb path${i}.ts`)
    }
    const bytes = Buffer.from(`${lines.join('\n')}\n`)

    const napi = JSON.parse(git.parseStatusPorcelain(bytes, limit))
    const ts = tsStatusOneShot(bytes, limit)

    expect(napi).toEqual(ts)
    expect(napi.entries.length).toBe(limit)
    expect(napi.didHitLimit).toBe(true)
    // The cap stops the scan one entry past the limit; statusLength is NOT 150000.
    expect(napi.statusLength).toBe(limit + 1)
    expect(napi.statusLength).not.toBe(total)
  })

  describe('numstat', () => {
    for (const fixture of numstatFixtures) {
      it(fixture.name, () => {
        const napi = JSON.parse(git.parseNumstat(fixture.bytes))
        expect(napi).toEqual(tsNumstatShape(fixture.bytes))
      })
    }
  })

  describe('parseWorktreeList', () => {
    for (const fixture of worktreeFixtures) {
      it(fixture.name, () => {
        const napi = JSON.parse(git.parseWorktreeList(fixture.output, fixture.nulDelimited))
        expect(napi).toEqual(
          parseWorktreeListTs(fixture.output, { nulDelimited: fixture.nulDelimited })
        )
      })
    }
  })

  describe('parseGitHistoryLog', () => {
    const US = '\x1f' // decoration separator (GIT_HISTORY_DECORATION_SEPARATOR)
    const rec = (fields: string[]): string => fields.join('\n')
    // A NUL-terminated `git log -z` stream, with the optional leading blank line
    // git emits before the first record.
    const stream = (...recs: string[]): string => recs.map((r) => `${r}\0`).join('')
    const historyFixtures: { name: string; stdout: string }[] = [
      {
        name: 'two commits, branch + tag + remote decorations, multiline message',
        stdout: `\n${stream(
          rec([
            'a'.repeat(40),
            'Ada L',
            'ada@x.io',
            '1700000000',
            '1700000001',
            'b'.repeat(40),
            `HEAD -> refs/heads/main${US}tag: refs/tags/v1${US}refs/remotes/origin/main`,
            'feat: subject\n\nbody'
          ]),
          rec(['c'.repeat(40), '', '', 'notanumber', '0', '', '', 'second'])
        )}`
      },
      { name: 'empty input', stdout: '' },
      {
        name: 'no decorations, single parent',
        stdout: stream(rec(['d'.repeat(40), 'B', 'b@y', '1', '2', 'e'.repeat(40), '', 'msg']))
      },
      {
        name: 'non-hash record is skipped',
        stdout: stream(rec(['not-a-hash', 'X', 'x@z', '1', '2', '', '', 'skip me']))
      }
    ]
    for (const fixture of historyFixtures) {
      it(fixture.name, () => {
        const napi = JSON.parse(git.parseGitHistoryLog(fixture.stdout))
        expect(napi).toEqual(parseGitHistoryLog(fixture.stdout))
      })
    }
  })

  describe('decodeGitCQuotedPath', () => {
    for (const value of decodeFixtures) {
      it(JSON.stringify(value), () => {
        expect(git.decodeGitCQuotedPath(value)).toBe(decodeGitCQuotedPath(value))
      })
    }
  })

  describe('countAdditionsInBuffer', () => {
    for (const fixture of countFixtures) {
      it(fixture.name, () => {
        expect(git.countAdditionsInBuffer(fixture.bytes)).toBe(tsCountAdditions(fixture.bytes))
      })
    }
  })

  describe('computeLineStats', () => {
    for (const fixture of lineStatsFixtures) {
      it(fixture.name, () => {
        const raw = git.computeLineStats(fixture.original, fixture.modified, fixture.status)
        const napi = raw === null ? null : JSON.parse(raw)
        expect(napi).toEqual(tsComputeLineStats(fixture.original, fixture.modified, fixture.status))
      })
    }
  })

  describe('validateGitPushTargetRules', () => {
    // Reuse the differential goldens the parity harness already runs against
    // orca_core::git_push_target — this proves the napi export matches BOTH the
    // pure TS validator and the recorded expectations for the value rules (the
    // unknown→typed guards are a JS-only concern, so vectors carry typed inputs).
    const vectors = JSON.parse(
      readFileSync(
        new URL('../../../tools/parity/vectors/git-push-target.json', import.meta.url),
        'utf8'
      )
    ) as {
      cases: {
        note: string
        input: { remoteName: string; branchName: string; remoteUrl?: string }
        expected: { ok: boolean; error?: string }
      }[]
    }

    const tsValueRule = (rn: string, bn: string, url: string | null): string | null => {
      try {
        assertGitPushTargetShape(
          url === null
            ? { remoteName: rn, branchName: bn }
            : { remoteName: rn, branchName: bn, remoteUrl: url }
        )
        return null
      } catch (error) {
        return error instanceof Error ? error.message : String(error)
      }
    }

    for (const c of vectors.cases) {
      it(c.note, () => {
        const url = c.input.remoteUrl ?? null
        const napi = git.validateGitPushTargetRules(c.input.remoteName, c.input.branchName, url)
        expect(napi).toBe(c.expected.ok ? null : (c.expected.error ?? null))
        expect(napi).toBe(tsValueRule(c.input.remoteName, c.input.branchName, url))
      })
    }
  })
})
