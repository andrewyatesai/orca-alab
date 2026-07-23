import { describe, expect, it, vi } from 'vitest'
import { CLIPBOARD_IMAGE_MAX_SOURCE_BYTES } from '../../shared/clipboard-image'
import { readWindowsClipboardImageFileAsPng } from './clipboard-windows-image-file'

function image(png = Buffer.from([1, 2, 3])) {
  return {
    getSize: () => ({ height: 10, width: 10 }),
    isEmpty: () => false,
    toPNG: () => png
  }
}

// A 'Shell IDList Array' (CIDA) buffer whose leading UInt32LE encodes cItems.
function shellIdListArray(itemCount: number): Buffer {
  const value = Buffer.alloc(4 + 4 * (itemCount + 1))
  value.writeUInt32LE(itemCount)
  return value
}

// Route each clipboard format to its own buffer; default CIDA reports one item.
function clipboardReader(fileNameBuffer: Buffer, cida = shellIdListArray(1)) {
  return vi.fn((format: string) => (format === 'Shell IDList Array' ? cida : fileNameBuffer))
}

describe('readWindowsClipboardImageFileAsPng', () => {
  it('converts a copied Windows image file to bounded PNG bytes', async () => {
    const filePath = 'C:\\Users\\alice\\图片\\shot.PNG'
    const png = Buffer.from([4, 3, 2, 1])
    const readClipboardFormatBuffer = clipboardReader(Buffer.from(`${filePath}\0`, 'utf16le'))
    const statFile = vi.fn(async () => ({ isFile: () => true, size: 1024 }))
    const createImageFromPath = vi.fn(() => image(png) as never)

    await expect(
      readWindowsClipboardImageFileAsPng({
        platform: 'win32',
        readClipboardFormatBuffer,
        statFile,
        createImageFromPath
      })
    ).resolves.toEqual(png)

    expect(readClipboardFormatBuffer).toHaveBeenCalledWith('FileNameW')
    expect(readClipboardFormatBuffer).toHaveBeenCalledWith('Shell IDList Array')
    expect(statFile).toHaveBeenCalledWith(filePath)
    expect(createImageFromPath).toHaveBeenCalledWith(filePath)
  })

  it('treats an empty CIDA buffer as a legacy single-file copy', async () => {
    const filePath = 'C:\\Users\\alice\\shot.png'
    const png = Buffer.from([9, 8, 7])
    const statFile = vi.fn(async () => ({ isFile: () => true, size: 1024 }))

    await expect(
      readWindowsClipboardImageFileAsPng({
        platform: 'win32',
        readClipboardFormatBuffer: clipboardReader(
          Buffer.from(`${filePath}\0`, 'utf16le'),
          Buffer.alloc(0)
        ),
        statFile,
        createImageFromPath: vi.fn(() => image(png) as never)
      })
    ).resolves.toEqual(png)

    expect(statFile).toHaveBeenCalledWith(filePath)
  })

  it('rejects a multi-file selection reported by the CIDA count field', async () => {
    const statFile = vi.fn()
    const createImageFromPath = vi.fn()

    await expect(
      readWindowsClipboardImageFileAsPng({
        platform: 'win32',
        // FileNameW still exposes only the first path; the CIDA count reveals two.
        readClipboardFormatBuffer: clipboardReader(
          Buffer.from('C:\\Users\\alice\\one.png\0', 'utf16le'),
          shellIdListArray(2)
        ),
        statFile,
        createImageFromPath
      })
    ).resolves.toBeNull()

    expect(statFile).not.toHaveBeenCalled()
    expect(createImageFromPath).not.toHaveBeenCalled()
  })

  it('ignores copied files outside Windows and unsupported file types', async () => {
    const statFile = vi.fn()
    const createImageFromPath = vi.fn()

    await expect(
      readWindowsClipboardImageFileAsPng({
        platform: 'linux',
        readClipboardFormatBuffer: clipboardReader(Buffer.from('/tmp/shot.png\0', 'utf16le')),
        statFile,
        createImageFromPath
      })
    ).resolves.toBeNull()
    await expect(
      readWindowsClipboardImageFileAsPng({
        platform: 'win32',
        readClipboardFormatBuffer: clipboardReader(
          Buffer.from('C:\\Users\\alice\\notes.txt\0', 'utf16le')
        ),
        statFile,
        createImageFromPath
      })
    ).resolves.toBeNull()

    expect(statFile).not.toHaveBeenCalled()
    expect(createImageFromPath).not.toHaveBeenCalled()
  })

  it('rejects embedded nulls instead of treating multiple paths as one file', async () => {
    const statFile = vi.fn()

    await expect(
      readWindowsClipboardImageFileAsPng({
        platform: 'win32',
        readClipboardFormatBuffer: clipboardReader(
          Buffer.from('C:\\Users\\alice\\one.png\0C:\\Users\\alice\\two.png\0', 'utf16le')
        ),
        statFile,
        createImageFromPath: vi.fn()
      })
    ).resolves.toBeNull()

    expect(statFile).not.toHaveBeenCalled()
  })

  it('rejects oversized source files before decoding them', async () => {
    const createImageFromPath = vi.fn()

    await expect(
      readWindowsClipboardImageFileAsPng({
        platform: 'win32',
        readClipboardFormatBuffer: clipboardReader(
          Buffer.from('C:\\Users\\alice\\huge.png\0', 'utf16le')
        ),
        statFile: vi.fn(async () => ({
          isFile: () => true,
          size: CLIPBOARD_IMAGE_MAX_SOURCE_BYTES + 1
        })),
        createImageFromPath
      })
    ).rejects.toThrow('Clipboard image is too large')

    expect(createImageFromPath).not.toHaveBeenCalled()
  })

  it('ignores clipboard files that disappeared before paste', async () => {
    const createImageFromPath = vi.fn()

    await expect(
      readWindowsClipboardImageFileAsPng({
        platform: 'win32',
        readClipboardFormatBuffer: clipboardReader(
          Buffer.from('C:\\Users\\alice\\missing.png\0', 'utf16le')
        ),
        statFile: vi.fn(async () => {
          throw new Error('ENOENT')
        }),
        createImageFromPath
      })
    ).resolves.toBeNull()

    expect(createImageFromPath).not.toHaveBeenCalled()
  })
})
