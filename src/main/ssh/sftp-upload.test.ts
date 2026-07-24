import { mkdtemp, mkdir, realpath, symlink, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { Writable } from 'node:stream'
import { describe, expect, it, vi } from 'vitest'
import type { SFTPWrapper } from 'ssh2'
import { removeDirectorySftp, uploadBuffer, uploadDirectory, uploadFile } from './sftp-upload'

function createWritable(): Writable {
  return new Writable({
    write(_chunk, _encoding, callback) {
      callback()
    }
  })
}

function createSftpMock(): SFTPWrapper {
  return {
    mkdir: vi.fn((_path: string, cb: (err?: Error | null) => void) => cb(null)),
    createWriteStream: vi.fn(() => createWritable()),
    readdir: vi.fn((_path: string, cb: (err?: Error | null, entries?: unknown[]) => void) =>
      cb(null, [])
    ),
    unlink: vi.fn((_path: string, cb: (err?: Error | null) => void) => cb(null)),
    rmdir: vi.fn((_path: string, cb: (err?: Error | null) => void) => cb(null)),
    rename: vi.fn((_from: string, _to: string, cb: (err?: Error | null) => void) => cb(null)),
    ext_openssh_rename: vi.fn((_from: string, _to: string, cb: (err?: Error | null) => void) =>
      cb(null)
    )
  } as unknown as SFTPWrapper
}

const partialOf = (finalPath: string): RegExp =>
  new RegExp(`^${finalPath.replace(/[.]/g, '\\.')}\\.orca-partial-`)

describe('sftp-upload', () => {
  it('can create the first binary upload chunk without clobbering an existing temp file', async () => {
    const sftp = createSftpMock()

    await uploadBuffer(sftp, Buffer.from('png'), '/remote/.logo.orca-upload', {
      exclusive: true
    })

    expect(sftp.createWriteStream).toHaveBeenCalledWith('/remote/.logo.orca-upload', {
      flags: 'wx'
    })
    const writeStream = vi.mocked(sftp.createWriteStream).mock.results[0]?.value as Writable
    expect(writeStream.listenerCount('close')).toBe(0)
    expect(writeStream.listenerCount('error')).toBe(0)
  })

  it('uses no-clobber writes for nested files during exclusive directory upload', async () => {
    const localDir = await mkdtemp(join(tmpdir(), 'orca-sftp-upload-'))
    await mkdir(join(localDir, 'nested'))
    await writeFile(join(localDir, 'nested', 'asset.txt'), 'asset')
    const sftp = createSftpMock()

    await uploadDirectory(sftp, localDir, '/remote/assets', await realpath(localDir), {
      exclusive: true
    })

    expect(sftp.mkdir).toHaveBeenCalledWith('/remote/assets/nested', expect.any(Function))
    // Why: the write lands on a temp sibling (created no-clobber), then is
    // renamed into the exclusive final path so a partial never appears there.
    expect(sftp.createWriteStream).toHaveBeenCalledWith(
      expect.stringMatching(partialOf('/remote/assets/nested/asset.txt')),
      { flags: 'wx' }
    )
    expect(sftp.rename).toHaveBeenCalledWith(
      expect.stringMatching(partialOf('/remote/assets/nested/asset.txt')),
      '/remote/assets/nested/asset.txt',
      expect.any(Function)
    )
    const writeStream = vi.mocked(sftp.createWriteStream).mock.results[0]?.value as Writable
    expect(writeStream.listenerCount('close')).toBe(0)
    expect(writeStream.listenerCount('error')).toBe(0)
  })

  it('uploads files from valid dot-dot-prefixed local directories', async () => {
    const localDir = await mkdtemp(join(tmpdir(), 'orca-sftp-upload-'))
    await mkdir(join(localDir, '..fixtures'))
    await writeFile(join(localDir, '..fixtures', 'asset.txt'), 'asset')
    const sftp = createSftpMock()

    await uploadDirectory(sftp, localDir, '/remote/assets', await realpath(localDir), {
      exclusive: true
    })

    expect(sftp.mkdir).toHaveBeenCalledWith('/remote/assets/..fixtures', expect.any(Function))
    expect(sftp.createWriteStream).toHaveBeenCalledWith(
      expect.stringMatching(partialOf('/remote/assets/..fixtures/asset.txt')),
      { flags: 'wx' }
    )
  })

  it('rejects sibling directories outside the upload root', async () => {
    const localDir = await mkdtemp(join(tmpdir(), 'orca-sftp-upload-'))
    const escapedDir = `${localDir}-sibling`
    await mkdir(escapedDir)
    await writeFile(join(escapedDir, 'asset.txt'), 'asset')
    const sftp = createSftpMock()

    await expect(
      uploadDirectory(sftp, escapedDir, '/remote/assets', await realpath(localDir), {
        exclusive: true
      })
    ).rejects.toThrow('Path escaped upload root')

    expect(sftp.mkdir).not.toHaveBeenCalled()
    expect(sftp.createWriteStream).not.toHaveBeenCalled()
  })

  it('does not create the remote file when the local source is a symlink', async () => {
    const localDir = await mkdtemp(join(tmpdir(), 'orca-sftp-upload-'))
    const targetPath = join(localDir, process.platform === 'win32' ? 'target-dir' : 'target.txt')
    const linkPath = join(localDir, process.platform === 'win32' ? 'link-dir' : 'link.txt')
    if (process.platform === 'win32') {
      await mkdir(targetPath)
      // Why: file symlinks often require Developer Mode/admin on Windows, while
      // junctions still exercise the symlink rejection branch.
      await symlink(targetPath, linkPath, 'junction')
    } else {
      await writeFile(targetPath, 'secret')
      await symlink(targetPath, linkPath)
    }
    const sftp = createSftpMock()

    await expect(uploadFile(sftp, linkPath, '/remote/link.txt')).rejects.toThrow()

    expect(sftp.createWriteStream).not.toHaveBeenCalled()
  })

  it('leaves no file at the final path and cleans the temp when the write fails mid-transfer', async () => {
    const localDir = await mkdtemp(join(tmpdir(), 'orca-sftp-upload-'))
    const localPath = join(localDir, 'src.txt')
    await writeFile(localPath, 'hello world')
    const created: string[] = []
    const renamed: [string, string][] = []
    const unlinked: string[] = []
    const sftp = {
      createWriteStream: vi.fn((p: string) => {
        created.push(p)
        return new Writable({
          write(_chunk, _encoding, callback) {
            // Simulate a disconnect mid-stream.
            callback(new Error('connection lost'))
          }
        })
      }),
      rename: vi.fn((from: string, to: string, cb: (err?: Error | null) => void) => {
        renamed.push([from, to])
        cb(null)
      }),
      ext_openssh_rename: vi.fn((_from: string, _to: string, cb: (err?: Error | null) => void) =>
        cb(new Error('unsupported'))
      ),
      unlink: vi.fn((p: string, cb: (err?: Error | null) => void) => {
        unlinked.push(p)
        cb(null)
      })
    } as unknown as SFTPWrapper

    await expect(uploadFile(sftp, localPath, '/remote/dest.txt')).rejects.toThrow()

    // Never promoted a partial into the final path...
    expect(renamed).toHaveLength(0)
    // ...wrote to a temp sibling, not the destination...
    expect(created[0]).not.toBe('/remote/dest.txt')
    expect(created[0]).toMatch(partialOf('/remote/dest.txt'))
    // ...and cleaned the staged temp so nothing lingers on the host.
    expect(unlinked).toContain(created[0])
  })

  it('promotes the staged temp to the final path only after a clean upload', async () => {
    const localDir = await mkdtemp(join(tmpdir(), 'orca-sftp-upload-'))
    const localPath = join(localDir, 'src.txt')
    await writeFile(localPath, 'payload')
    const sftp = createSftpMock()

    await uploadFile(sftp, localPath, '/remote/dest.txt', { exclusive: true })

    const created = vi.mocked(sftp.createWriteStream).mock.calls[0]?.[0] as string
    expect(created).toMatch(partialOf('/remote/dest.txt'))
    expect(sftp.rename).toHaveBeenCalledWith(created, '/remote/dest.txt', expect.any(Function))
  })

  it('removes remote directory contents before removing the directory', async () => {
    const sftp = createSftpMock()
    vi.mocked(sftp.readdir).mockImplementation((remotePath, cb) => {
      const pathString = String(remotePath)
      if (pathString === '/remote/assets') {
        cb(undefined, [
          { filename: '.', attrs: { isDirectory: () => true } },
          { filename: '..', attrs: { isDirectory: () => true } },
          { filename: 'nested', attrs: { isDirectory: () => true } },
          { filename: 'logo.png', attrs: { isDirectory: () => false } }
        ] as never)
        return
      }
      if (pathString === '/remote/assets/nested') {
        cb(undefined, [{ filename: 'copy.txt', attrs: { isDirectory: () => false } }] as never)
        return
      }
      cb(new Error(`unexpected readdir: ${pathString}`), [] as never)
    })

    await removeDirectorySftp(sftp, '/remote/assets')

    expect(sftp.unlink).toHaveBeenNthCalledWith(
      1,
      '/remote/assets/nested/copy.txt',
      expect.any(Function)
    )
    expect(sftp.rmdir).toHaveBeenNthCalledWith(1, '/remote/assets/nested', expect.any(Function))
    expect(sftp.unlink).toHaveBeenNthCalledWith(2, '/remote/assets/logo.png', expect.any(Function))
    expect(sftp.rmdir).toHaveBeenNthCalledWith(2, '/remote/assets', expect.any(Function))
  })
})
