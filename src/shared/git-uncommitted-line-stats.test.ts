import { beforeEach, describe, expect, it, vi, type Mock } from 'vitest'

const { lstatMock, readFileMock } = vi.hoisted(() => ({
  lstatMock: vi.fn(),
  readFileMock: vi.fn()
}))

vi.mock('fs/promises', () => ({ lstat: lstatMock, readFile: readFileMock }))

import {
  applyLineStats,
  collectUntrackedAdditions,
  MAX_UNTRACKED_LINE_COUNT_BYTES,
  MAX_UNTRACKED_LINE_COUNT_FILES
} from './git-uncommitted-line-stats'

function mockFileStat(size: number, mtimeMs = 1) {
  return {
    size,
    mtimeMs,
    ctimeMs: mtimeMs,
    isFile: () => true,
    isSymbolicLink: () => false
  }
}

// The parseNumstat tests were removed with the TS parser: numstat parsing is now
// the Rust orca_git::numstat core (napi in main, wasm in the relay), covered by
// orca-git's unit tests and the relay's git-wasm path.

describe('collectUntrackedAdditions', () => {
  // The byte-counting algorithm now lives in Rust (orca-git count_additions_in_buffer,
  // proven by orca-git-napi-parity.test.ts); these tests cover the TS ORCHESTRATION —
  // IO, cache, symlink/oversize skips — with the counter injected as a mock.
  let countAdditions: Mock<(buffer: Buffer) => number | null>
  beforeEach(() => {
    lstatMock.mockReset()
    readFileMock.mockReset()
    countAdditions = vi.fn<(buffer: Buffer) => number | null>()
  })

  it('returns the injected counter result as the added count, per read file', async () => {
    lstatMock.mockResolvedValue(mockFileStat(5))
    readFileMock.mockResolvedValue(Buffer.from('a\nb\nc'))
    countAdditions.mockReturnValue(3)
    const stats = await collectUntrackedAdditions('/repo', ['lines.ts'], countAdditions)
    expect(stats.get('lines.ts')).toEqual({ added: 3 })
    expect(countAdditions).toHaveBeenCalledTimes(1)
  })

  it('reports zero additions when the counter returns 0 (empty file)', async () => {
    lstatMock.mockResolvedValue(mockFileStat(0))
    readFileMock.mockResolvedValue(Buffer.from(''))
    countAdditions.mockReturnValue(0)
    expect(
      (await collectUntrackedAdditions('/repo', ['empty.ts'], countAdditions)).get('empty.ts')
    ).toEqual({ added: 0 })
  })

  it('omits counts when the counter returns null (binary)', async () => {
    lstatMock.mockResolvedValue(mockFileStat(3))
    readFileMock.mockResolvedValue(Buffer.from([0x00, 0x01, 0x02]))
    countAdditions.mockReturnValue(null)
    expect(
      (await collectUntrackedAdditions('/repo', ['bin.dat'], countAdditions)).get('bin.dat')
    ).toEqual({})
  })

  it('counts untracked symbolic links as one addition without reading or counting', async () => {
    lstatMock.mockResolvedValue({
      size: 4,
      mtimeMs: 2,
      ctimeMs: 2,
      isFile: () => false,
      isSymbolicLink: () => true
    })
    expect(
      (await collectUntrackedAdditions('/repo', ['link.txt'], countAdditions)).get('link.txt')
    ).toEqual({ added: 1 })
    expect(readFileMock).not.toHaveBeenCalled()
    expect(countAdditions).not.toHaveBeenCalled()
  })

  it('skips oversized untracked files instead of reading them during status polling', async () => {
    lstatMock.mockResolvedValue(mockFileStat(MAX_UNTRACKED_LINE_COUNT_BYTES + 1, 3))
    expect(
      (await collectUntrackedAdditions('/repo', ['large.log'], countAdditions)).get('large.log')
    ).toEqual({})
    expect(readFileMock).not.toHaveBeenCalled()
  })

  it('reuses cached counts while size and mtime are unchanged', async () => {
    lstatMock.mockResolvedValue(mockFileStat(5, 4))
    readFileMock.mockResolvedValue(Buffer.from('a\nb\nc'))
    countAdditions.mockReturnValue(3)
    await collectUntrackedAdditions('/repo', ['cached.ts'], countAdditions)
    const stats = await collectUntrackedAdditions('/repo', ['cached.ts'], countAdditions)
    expect(stats.get('cached.ts')).toEqual({ added: 3 })
    expect(readFileMock).toHaveBeenCalledTimes(1)
    expect(countAdditions).toHaveBeenCalledTimes(1)
  })

  it('skips untracked counting entirely when no counter is provided (e.g. the relay)', async () => {
    const stats = await collectUntrackedAdditions('/repo', ['x.ts'])
    expect(stats.size).toBe(0)
    expect(lstatMock).not.toHaveBeenCalled()
  })

  it('keeps the cache effective across polls for a max-counted change set', async () => {
    // Why: a cache smaller than one counted scan FIFO-evicts every entry
    // mid-scan, so the next poll re-reads every file (#8013). The counted set is
    // now capped at MAX_UNTRACKED_LINE_COUNT_FILES per poll; scan a full capped
    // set twice — the second pass must be stat-only.
    lstatMock.mockResolvedValue(mockFileStat(5, 7))
    readFileMock.mockResolvedValue(Buffer.from('a\nb\nc'))
    countAdditions.mockReturnValue(3)
    const paths = Array.from(
      { length: MAX_UNTRACKED_LINE_COUNT_FILES },
      (_, i) => `poll-scale/file-${i}.ts`
    )

    // The fork gates counting on an explicit counter (the relay passes none);
    // supply one so the scan reads + caches, then verify the cache survives a
    // full capped scan (no re-read on the second poll).
    await collectUntrackedAdditions('/repo', paths, countAdditions)
    const firstPassReads = readFileMock.mock.calls.length
    await collectUntrackedAdditions('/repo', paths, countAdditions)

    expect(firstPassReads).toBe(paths.length)
    expect(readFileMock).toHaveBeenCalledTimes(paths.length)
  })

  it('counts every untracked file at the file-count cap boundary', async () => {
    lstatMock.mockResolvedValue(mockFileStat(5, 9))
    readFileMock.mockResolvedValue(Buffer.from('a\nb\nc'))
    countAdditions.mockReturnValue(3)
    const paths = Array.from(
      { length: MAX_UNTRACKED_LINE_COUNT_FILES },
      (_, i) => `at-cap/file-${i}.ts`
    )
    const stats = await collectUntrackedAdditions('/repo', paths, countAdditions)
    expect(stats.size).toBe(MAX_UNTRACKED_LINE_COUNT_FILES)
    expect(countAdditions).toHaveBeenCalledTimes(MAX_UNTRACKED_LINE_COUNT_FILES)
  })

  it('skips untracked counting above the file-count cap without any lstat or read', async () => {
    // Why: the middle-band (100-9,999 untracked files) generated/dependency dir
    // must not lstat/read every file each poll; over the cap we skip entirely.
    const paths = Array.from(
      { length: MAX_UNTRACKED_LINE_COUNT_FILES + 1 },
      (_, i) => `over-cap/file-${i}.ts`
    )
    const stats = await collectUntrackedAdditions('/repo', paths, countAdditions)
    expect(stats.size).toBe(0)
    expect(lstatMock).not.toHaveBeenCalled()
    expect(readFileMock).not.toHaveBeenCalled()
    expect(countAdditions).not.toHaveBeenCalled()
  })
})

describe('applyLineStats', () => {
  it('copies defined counts onto the entry', () => {
    const entry: { added?: number; removed?: number } = {}
    applyLineStats(entry, { added: 5, removed: 2 })
    expect(entry).toEqual({ added: 5, removed: 2 })
  })

  it('leaves the entry untouched for undefined counts or missing stats', () => {
    const entry: { added?: number; removed?: number } = {}
    applyLineStats(entry, { added: undefined, removed: undefined })
    applyLineStats(entry, undefined)
    expect(entry).toEqual({})
  })
})
