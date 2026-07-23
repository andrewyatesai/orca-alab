import { describe, expect, it } from 'vitest'
import { getUpdateInstallMode } from './updater-install-policy'

describe('updater install policy', () => {
  it.each(['darwin', 'win32'] satisfies NodeJS.Platform[])(
    'uses manual releases on %s until ALab has a stable publisher identity',
    (platform) => {
      expect(getUpdateInstallMode(platform)).toBe('manual')
    }
  )

  it('keeps automatic update installation on Linux', () => {
    expect(getUpdateInstallMode('linux')).toBe('automatic')
  })
})
