import { afterEach, describe, expect, it, vi } from 'vitest'

const { getMediaAccessStatusMock, askForMediaAccessMock } = vi.hoisted(() => ({
  getMediaAccessStatusMock: vi.fn(),
  askForMediaAccessMock: vi.fn()
}))

vi.mock('electron', () => ({
  systemPreferences: {
    getMediaAccessStatus: getMediaAccessStatusMock,
    askForMediaAccess: askForMediaAccessMock
  }
}))

import type { MediaAccessPermissionRequest } from 'electron'
import { hasSystemMediaAccess, requestSystemMediaAccess } from './browser-media-access'

function mediaRequest(mediaTypes: ('audio' | 'video')[]): MediaAccessPermissionRequest {
  return {
    mediaTypes,
    isMainFrame: true,
    requestingUrl: 'https://example.com/'
  } as MediaAccessPermissionRequest
}

async function withPlatform(platform: NodeJS.Platform, fn: () => Promise<void>): Promise<void> {
  const original = Object.getOwnPropertyDescriptor(process, 'platform')
  Object.defineProperty(process, 'platform', { value: platform, configurable: true })
  try {
    await fn()
  } finally {
    if (original) {
      Object.defineProperty(process, 'platform', original)
    }
  }
}

describe('browser media access', () => {
  afterEach(() => {
    getMediaAccessStatusMock.mockReset()
    askForMediaAccessMock.mockReset()
  })

  it('fails closed off-darwin instead of blanket-granting every origin', async () => {
    // Regression: returning true here handed camera/mic to any origin on
    // Linux/Windows with no prompt and no per-origin gate.
    await withPlatform('linux', async () => {
      expect(hasSystemMediaAccess('video')).toBe(false)
      expect(hasSystemMediaAccess('audio')).toBe(false)
      await expect(requestSystemMediaAccess(mediaRequest(['video']))).resolves.toBe(false)
      expect(askForMediaAccessMock).not.toHaveBeenCalled()
    })
    await withPlatform('win32', async () => {
      expect(hasSystemMediaAccess('video')).toBe(false)
      await expect(requestSystemMediaAccess(mediaRequest(['audio']))).resolves.toBe(false)
    })
  })

  it('gates on macOS TCC status on darwin', async () => {
    await withPlatform('darwin', async () => {
      getMediaAccessStatusMock.mockReturnValue('granted')
      expect(hasSystemMediaAccess('video')).toBe(true)
      getMediaAccessStatusMock.mockReturnValue('denied')
      expect(hasSystemMediaAccess('audio')).toBe(false)

      askForMediaAccessMock.mockResolvedValue(true)
      await expect(requestSystemMediaAccess(mediaRequest(['video']))).resolves.toBe(true)
      expect(askForMediaAccessMock).toHaveBeenCalledWith('camera')
    })
  })
})
