import { beforeEach, describe, expect, it, vi } from 'vitest'
import { basename, join } from 'node:path'

const { fsMkdtempMock, fsReaddirMock, fsRmMock, fsStatMock, fsWriteFileMock, randomUUIDMock } =
  vi.hoisted(() => ({
    fsMkdtempMock: vi.fn(),
    fsReaddirMock: vi.fn(),
    fsRmMock: vi.fn(),
    fsStatMock: vi.fn(),
    fsWriteFileMock: vi.fn(),
    randomUUIDMock: vi.fn(() => '00000000-0000-4000-8000-000000000000')
  }))

const requireSshFilesystemProviderMock = vi.hoisted(() => vi.fn())

vi.mock('node:fs/promises', () => ({
  default: {
    mkdtemp: fsMkdtempMock,
    readdir: fsReaddirMock,
    rm: fsRmMock,
    stat: fsStatMock,
    writeFile: fsWriteFileMock
  }
}))

vi.mock('node:crypto', () => ({
  randomUUID: randomUUIDMock
}))

vi.mock('electron', () => ({
  app: {
    getPath: vi.fn(() => '/tmp')
  }
}))

vi.mock('../providers/ssh-filesystem-dispatch', () => ({
  requireSshFilesystemProvider: requireSshFilesystemProviderMock
}))

import {
  cleanupExpiredClipboardImageTempDirs,
  saveClipboardImageBufferAsTempFile
} from './clipboard-image-temp-file'

const SEVEN_DAYS_MS = 7 * 24 * 60 * 60 * 1000

function dirent(name: string, directory = true): { name: string; isDirectory: () => boolean } {
  return { name, isDirectory: () => directory }
}

describe('saveClipboardImageBufferAsTempFile', () => {
  beforeEach(() => {
    vi.spyOn(Date, 'now').mockReturnValue(1760000000000)
    fsMkdtempMock.mockReset()
    fsMkdtempMock.mockImplementation(async (prefix: string) => `${prefix}a1b2c3`)
    fsReaddirMock.mockReset()
    fsRmMock.mockReset()
    fsRmMock.mockResolvedValue(undefined)
    fsStatMock.mockReset()
    fsWriteFileMock.mockReset()
    fsWriteFileMock.mockResolvedValue(undefined)
    randomUUIDMock.mockReset()
    randomUUIDMock.mockReturnValue('00000000-0000-4000-8000-000000000000')
    requireSshFilesystemProviderMock.mockReset()
  })

  it('writes local pastes into a fresh private mkdtemp directory', async () => {
    const png = Buffer.from([0, 1, 2, 3])

    const savedPath = await saveClipboardImageBufferAsTempFile(png)

    expect(fsMkdtempMock).toHaveBeenCalledWith(join('/tmp', 'orca-paste-'))
    expect(savedPath).toBe(
      join('/tmp', 'orca-paste-a1b2c3', 'orca-paste-00000000-0000-4000-8000-000000000000.png')
    )
    expect(fsWriteFileMock).toHaveBeenCalledWith(savedPath, png)
  })

  it('keeps the orca-paste basename prefix that paste detection relies on', async () => {
    const savedPath = await saveClipboardImageBufferAsTempFile(Buffer.from([0]))

    // Why: native-chat + transcript decoders detect pastes via /^orca-paste-.+\.png$/i.
    expect(/^orca-paste-.+\.png$/i.test(basename(savedPath))).toBe(true)
  })

  it('writes SSH pastes to the remote temp dir without touching local disk', async () => {
    const writeFileBase64 = vi.fn().mockResolvedValue(undefined)
    requireSshFilesystemProviderMock.mockReturnValue({
      getTempDir: vi.fn().mockResolvedValue('/var/tmp'),
      writeFileBase64
    })
    const png = Buffer.from([0, 1, 2, 3])

    await expect(saveClipboardImageBufferAsTempFile(png, { connectionId: 'ssh-1' })).resolves.toBe(
      '/var/tmp/orca-paste-1760000000000-00000000-0000-4000-8000-000000000000.png'
    )
    expect(writeFileBase64).toHaveBeenCalledWith(
      '/var/tmp/orca-paste-1760000000000-00000000-0000-4000-8000-000000000000.png',
      png.toString('base64')
    )
    expect(fsMkdtempMock).not.toHaveBeenCalled()
    expect(fsWriteFileMock).not.toHaveBeenCalled()
  })
})

describe('cleanupExpiredClipboardImageTempDirs', () => {
  beforeEach(() => {
    fsReaddirMock.mockReset()
    fsRmMock.mockReset()
    fsRmMock.mockResolvedValue(undefined)
    fsStatMock.mockReset()
  })

  it('removes only orca-paste directories older than seven days', async () => {
    const nowMs = 1760000000000
    fsReaddirMock.mockResolvedValue([
      dirent('orca-paste-expired'),
      dirent('orca-paste-fresh'),
      dirent('orca-paste-legacy-file.png', false),
      dirent('orca-clipboard-file-expired'),
      dirent('unrelated-temp')
    ])
    fsStatMock.mockImplementation(async (targetPath: string) => {
      if (targetPath.endsWith('orca-paste-expired')) {
        return { mtimeMs: nowMs - SEVEN_DAYS_MS - 1 }
      }
      if (targetPath.endsWith('orca-paste-fresh')) {
        return { mtimeMs: nowMs - SEVEN_DAYS_MS + 1000 }
      }
      throw new Error(`unexpected stat: ${targetPath}`)
    })

    await cleanupExpiredClipboardImageTempDirs(nowMs)

    expect(fsRmMock).toHaveBeenCalledTimes(1)
    expect(fsRmMock).toHaveBeenCalledWith(join('/tmp', 'orca-paste-expired'), {
      recursive: true,
      force: true
    })
  })

  it('is a no-op when the temp root cannot be listed', async () => {
    fsReaddirMock.mockRejectedValue(new Error('EACCES'))

    await expect(cleanupExpiredClipboardImageTempDirs()).resolves.toBeUndefined()
    expect(fsRmMock).not.toHaveBeenCalled()
  })

  it('still sweeps remaining dirs when one entry fails to stat or remove', async () => {
    const nowMs = 1760000000000
    fsReaddirMock.mockResolvedValue([dirent('orca-paste-broken'), dirent('orca-paste-expired')])
    fsStatMock.mockImplementation(async (targetPath: string) => {
      if (targetPath.endsWith('broken')) {
        throw new Error('EPERM')
      }
      return { mtimeMs: nowMs - SEVEN_DAYS_MS - 1 }
    })

    await expect(cleanupExpiredClipboardImageTempDirs(nowMs)).resolves.toBeUndefined()
    expect(fsRmMock).toHaveBeenCalledTimes(1)
    expect(fsRmMock).toHaveBeenCalledWith(join('/tmp', 'orca-paste-expired'), {
      recursive: true,
      force: true
    })
  })
})
