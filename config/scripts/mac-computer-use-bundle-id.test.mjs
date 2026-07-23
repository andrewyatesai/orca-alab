import { describe, expect, it } from 'vitest'
import {
  ALAB_COMPUTER_USE_BUNDLE_ID,
  PUBLIC_COMPUTER_USE_BUNDLE_ID,
  resolveMacComputerUseBundleId
} from './mac-computer-use-bundle-id.mjs'

describe('mac computer-use bundle identity', () => {
  it('keeps ALab TCC identity separate from production', () => {
    expect(resolveMacComputerUseBundleId({})).toBe(ALAB_COMPUTER_USE_BUNDLE_ID)
    expect(ALAB_COMPUTER_USE_BUNDLE_ID).not.toBe(PUBLIC_COMPUTER_USE_BUNDLE_ID)
  })

  it('uses production identity only for an explicit public-identity build', () => {
    expect(resolveMacComputerUseBundleId({ ORCA_PUBLIC_IDENTITY: '1' })).toBe(
      PUBLIC_COMPUTER_USE_BUNDLE_ID
    )
  })

  it('honors an explicit helper identity override', () => {
    expect(
      resolveMacComputerUseBundleId({
        ORCA_PUBLIC_IDENTITY: '1',
        ORCA_COMPUTER_MACOS_BUNDLE_ID: 'com.example.custom-helper'
      })
    ).toBe('com.example.custom-helper')
  })
})
