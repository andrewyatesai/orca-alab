import { Readable, Writable } from 'node:stream'
import type { SFTPWrapper } from 'ssh2'
import { beforeEach, describe, expect, it, vi } from 'vitest'

// Why: this exercises the REAL upload primitive (no mock of uploadFile itself),
// mocking only node:fs/promises (local source) and the ssh2 SFTP layer, so it
// FAILS if sftp-upload.ts is reverted from atomic temp+rename to writing
// straight at remotePath. It pins the caller-facing fail-closed contract that
// the filesystem-import-ssh finding depends on: a mid-transfer failure never
// lands a partial at destPath.
const { lstatMock, openMock } = vi.hoisted(() => ({
  lstatMock: vi.fn(),
  openMock: vi.fn()
}))

vi.mock('node:fs/promises', () => ({
  lstat: lstatMock,
  open: openMock,
  readdir: vi.fn(),
  realpath: vi.fn()
}))

import { uploadFile } from '../ssh/sftp-upload'

const DEST = '/home/user/project/report.txt'
const PARTIAL = /\.orca-partial-[0-9a-f-]+$/

function mockLocalSource(): void {
  const meta = { size: 4, ino: 1, dev: 1 }
  lstatMock.mockResolvedValue({
    ...meta,
    isFile: () => true,
    isDirectory: () => false,
    isSymbolicLink: () => false
  })
  openMock.mockResolvedValue({
    close: vi.fn().mockResolvedValue(undefined),
    stat: vi.fn().mockResolvedValue({ ...meta, isFile: () => true }),
    createReadStream: () => Readable.from([Buffer.from('data')])
  })
}

type SftpMock = {
  sftp: SFTPWrapper
  createWriteStream: ReturnType<typeof vi.fn>
  rename: ReturnType<typeof vi.fn>
  unlink: ReturnType<typeof vi.fn>
}

function makeSftp(writeStream: Writable): SftpMock {
  const createWriteStream = vi.fn(() => writeStream)
  const rename = vi.fn((_from: string, _to: string, cb: (e: unknown) => void) => cb(null))
  const unlink = vi.fn((_p: string, cb: (e: unknown) => void) => cb(null))
  const ext_openssh_rename = vi.fn((_f: string, _t: string, cb: (e: unknown) => void) => cb(null))
  return {
    sftp: { createWriteStream, rename, unlink, ext_openssh_rename } as unknown as SFTPWrapper,
    createWriteStream,
    rename,
    unlink
  }
}

describe('uploadFile — atomic temp+rename staging', () => {
  beforeEach(() => {
    lstatMock.mockReset()
    openMock.mockReset()
    mockLocalSource()
  })

  it('promotes to destPath only via rename from a temp sibling, never writes destPath directly', async () => {
    // A Writable that accepts the data and finishes cleanly (successful upload).
    const ws = new Writable({ write: (_c, _e, cb) => cb() })
    const m = makeSftp(ws)

    await uploadFile(m.sftp, '/tmp/report.txt', DEST, { exclusive: true })

    // (a) the write target is a temp sibling, NOT destPath directly.
    const writeTarget = m.createWriteStream.mock.calls[0][0] as string
    expect(writeTarget).not.toBe(DEST)
    expect(writeTarget).toMatch(PARTIAL)
    expect(writeTarget.startsWith(`${DEST}.orca-partial-`)).toBe(true)
    // (b) destPath is reached only by renaming the fully-written temp into place.
    expect(m.rename).toHaveBeenCalledWith(writeTarget, DEST, expect.any(Function))
  })

  it('leaves destPath untouched and unlinks the temp when the transfer fails mid-stream', async () => {
    // A Writable that rejects the first chunk — simulates a mid-transfer
    // disconnect / write error.
    const ws = new Writable({ write: (_c, _e, cb) => cb(new Error('read ECONNRESET')) })
    const m = makeSftp(ws)

    await expect(uploadFile(m.sftp, '/tmp/report.txt', DEST, { exclusive: true })).rejects.toThrow(
      'read ECONNRESET'
    )

    const writeTarget = m.createWriteStream.mock.calls[0][0] as string
    // The write only ever targeted the temp sibling...
    expect(writeTarget).toMatch(PARTIAL)
    expect(writeTarget).not.toBe(DEST)
    // ...nothing was promoted to destPath...
    expect(m.rename).not.toHaveBeenCalled()
    // ...and the partial temp was cleaned up, so no orphan lingers.
    expect(m.unlink).toHaveBeenCalledWith(writeTarget, expect.any(Function))
    // destPath itself is never unlinked (we don't own a racing foreign file).
    for (const [p] of m.unlink.mock.calls) {
      expect(p).not.toBe(DEST)
    }
  })
})
