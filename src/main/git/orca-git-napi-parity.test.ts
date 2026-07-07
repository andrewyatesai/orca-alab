import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { loadRustGitBinding } from '../daemon/rust-git-addon'
import { decodeGitCQuotedPath } from '../../shared/git-cquoted-path'
import { assertGitPushTargetShape } from '../../shared/git-push-target-validation'
import { isBinaryBuffer } from '../../shared/binary-buffer'

// napi-surface checks for the Rust `orca-git` parsers. The dual-run TS oracles
// were retired as each TS parser was deleted (the Rust core is the sole impl —
// napi in main, wasm in the relay); the parser logic itself is covered by
// orca-git's unit tests and the relay's differential tests. What remains here:
// status streaming↔one-shot self-consistency, parity against the STILL-LIVE
// shared TS at the JS boundary (decodeGitCQuotedPath, push-target shape), a
// transcribed golden for count (its inline TS loop was deleted), and a
// transcribed golden for line-stats (its TS original, computeLineStats, is
// still live in the renderer project — which this node project cannot import).
//
// Skips cleanly when the .node is absent (CI without a native build), so the
// suite still passes there.

const binding = loadRustGitBinding()
const suite = binding ? describe : describe.skip
// Safe under describe.skip: the closure never runs when the binding is null.
const git = binding!

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

suite('orca-git napi surface', () => {
  it('exposes the orca-git engine marker', () => {
    expect(git.gitEngine()).toBe('orca-git')
  })

  // Streaming (chunked) must agree with the one-shot scan for the same bytes +
  // cap — this pins the napi streaming surface (chunk carry, cap semantics,
  // lossy decode at chunk boundaries) against the one-shot scan the relay wasm
  // differential tests verify. Chunk size 5 splits every multi-byte fixture.
  describe('status porcelain streaming (chunked) agrees with the one-shot scan', () => {
    for (const fixture of statusFixtures) {
      it(fixture.name, () => {
        const napi = napiStatusStreaming(fixture.bytes, fixture.limit, 5)
        expect(napi).toEqual(JSON.parse(git.parseStatusPorcelain(fixture.bytes, fixture.limit)))
      })
    }
    for (const fixture of streamingFixtures) {
      it(fixture.name, () => {
        const napi = napiStatusStreaming(fixture.bytes, fixture.limit, fixture.chunkSize)
        expect(napi).toEqual(JSON.parse(git.parseStatusPorcelain(fixture.bytes, fixture.limit)))
      })
    }
  })

  it('pins C-quoted decoding on the untracked/ignored branches (absolute expectations)', () => {
    // Guards the decode_git_cquoted_path wiring in the `? `/`! ` record branches
    // with fixed expected paths — the streaming↔one-shot self-consistency check
    // above cannot catch a decode dropped from BOTH legs (they share parse_line).
    // Octal escapes decode per-byte (fromCharCode semantics, matching the live
    // shared TS decoder), so \303\251 pins to 'Ã©', not 'é'.
    const napi = JSON.parse(
      git.parseStatusPorcelain(Buffer.from('? "\\303\\251.txt"\n! "tab\\tname.log"\n'), 0)
    ) as { entries: { path: string }[]; ignoredPaths: string[] }
    expect(napi.entries.map((entry) => entry.path)).toEqual(['Ã©.txt'])
    expect(napi.ignoredPaths).toEqual(['tab\tname.log'])
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

    expect(napi.entries.length).toBe(limit)
    expect(napi.didHitLimit).toBe(true)
    // The cap stops the scan one entry past the limit; statusLength is NOT 150000.
    expect(napi.statusLength).toBe(limit + 1)
    expect(napi.statusLength).not.toBe(total)
  })

  // The status one-shot, numstat, parseWorktreeList, and parseGitHistoryLog TS
  // oracles were retired: those TS parsers were deleted once the Rust core became
  // the sole impl (napi in main, wasm in the relay). The Rust logic is covered by
  // orca-git's unit tests and the relay's differential tests (same wasm core).

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
