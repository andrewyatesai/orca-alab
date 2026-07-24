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
const DEST_DIR = '/home/user/project'
// Why: staged temp is a SHORT fixed-length sibling in the destination's parent
// directory, derived independently of the destination basename length.
const PARTIAL = /\.orca-tmp-[0-9a-f]+$/
const siblingIn = (dir: string): RegExp => new RegExp(`^${dir}/\\.orca-tmp-[0-9a-f]+$`)

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

    // (a) the write target is a temp sibling in the SAME parent dir, NOT destPath.
    const writeTarget = m.createWriteStream.mock.calls[0][0] as string
    expect(writeTarget).not.toBe(DEST)
    expect(writeTarget).toMatch(PARTIAL)
    expect(writeTarget).toMatch(siblingIn(DEST_DIR))
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

  // Pins cxs1: the staged basename must stay within the 255-byte component limit
  // even when the destination basename is itself near the limit. The old scheme
  // (`${dest}.orca-partial-<uuid>`) appended 50 bytes to the basename, overflowing
  // NAME_MAX and failing a VALID upload that worked before atomic staging landed.
  it('keeps the staged temp basename within NAME_MAX for a near-limit destination basename', async () => {
    // A valid 250-byte basename — under NAME_MAX (255) on its own.
    const longBase = 'a'.repeat(250)
    const longDest = `${DEST_DIR}/${longBase}`
    const ws = new Writable({ write: (_c, _e, cb) => cb() })
    const m = makeSftp(ws)

    await uploadFile(m.sftp, '/tmp/report.txt', longDest, { exclusive: true })

    const writeTarget = m.createWriteStream.mock.calls[0][0] as string
    const stagedBasename = writeTarget.slice(writeTarget.lastIndexOf('/') + 1)
    expect(Buffer.byteLength(stagedBasename)).toBeLessThanOrEqual(255)
    // And it still stages a sibling in the destination's parent directory.
    expect(writeTarget).toMatch(siblingIn(DEST_DIR))
  })

  // Pins cxs2: on a mid-stream failure the staged-temp REMOVE must complete
  // before the promise rejects, so the caller's immediate sftp.end() can't cancel
  // a still-pending unlink and orphan the temp. We hold the unlink callback and
  // assert the promise stays pending until the REMOVE actually completes.
  it('awaits the staged-temp unlink to completion before rejecting on failure', async () => {
    const ws = new Writable({ write: (_c, _e, cb) => cb(new Error('read ECONNRESET')) })
    const m = makeSftp(ws)
    // Hold the unlink callback so the REMOVE stays outstanding until we release it.
    let releaseUnlink: (() => void) | undefined
    m.unlink.mockImplementation((_p: string, cb: (e: unknown) => void) => {
      releaseUnlink = () => cb(null)
    })

    const uploadPromise = uploadFile(m.sftp, '/tmp/report.txt', DEST, { exclusive: true })
    let settled = false
    void uploadPromise.then(
      () => {
        settled = true
      },
      () => {
        settled = true
      }
    )

    // Let the mid-stream error fully propagate into the reject path (which issues
    // the unlink). A macrotask is far longer than the microtask chain involved.
    await new Promise((resolve) => setTimeout(resolve, 20))

    expect(m.unlink).toHaveBeenCalledWith(
      m.createWriteStream.mock.calls[0][0],
      expect.any(Function)
    )
    expect(releaseUnlink).toBeDefined()
    // With the fix the reject is gated on the held unlink, so the promise is still
    // pending. A bare `void unlink()` reject path would have already settled here.
    expect(settled).toBe(false)

    // Completing the REMOVE lets the upload promise finally reject.
    releaseUnlink?.()
    await expect(uploadPromise).rejects.toThrow('read ECONNRESET')
  })
})
