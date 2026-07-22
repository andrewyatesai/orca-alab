import { describe, expect, it } from 'vitest'
import {
  getFeatureWallRailNavigationTarget,
  normalizeFeatureWallRailNavigationKey
} from './feature-wall-rail-navigation'

describe('normalizeFeatureWallRailNavigationKey', () => {
  it('uses up and down arrows for the desktop vertical rail', () => {
    expect(normalizeFeatureWallRailNavigationKey('ArrowUp', 'vertical')).toBe('ArrowUp')
    expect(normalizeFeatureWallRailNavigationKey('ArrowLeft', 'vertical')).toBeNull()
  })

  it('maps left and right arrows for the compact horizontal row', () => {
    expect(normalizeFeatureWallRailNavigationKey('ArrowLeft', 'horizontal')).toBe('ArrowUp')
    expect(normalizeFeatureWallRailNavigationKey('ArrowRight', 'horizontal')).toBe('ArrowDown')
    expect(normalizeFeatureWallRailNavigationKey('ArrowDown', 'horizontal')).toBeNull()
  })

  it('keeps Home and End available in either orientation', () => {
    expect(normalizeFeatureWallRailNavigationKey('Home', 'horizontal')).toBe('Home')
    expect(normalizeFeatureWallRailNavigationKey('End', 'vertical')).toBe('End')
  })
})

describe('getFeatureWallRailNavigationTarget', () => {
  it('moves down past the current row', () => {
    expect(
      getFeatureWallRailNavigationTarget({ currentIndex: 0, key: 'ArrowDown', itemCount: 5 })
    ).toBe(1)
  })

  it('clamps ArrowDown at the last row', () => {
    expect(
      getFeatureWallRailNavigationTarget({ currentIndex: 4, key: 'ArrowDown', itemCount: 5 })
    ).toBe(4)
  })

  it('moves up past the current row', () => {
    expect(
      getFeatureWallRailNavigationTarget({ currentIndex: 3, key: 'ArrowUp', itemCount: 5 })
    ).toBe(2)
  })

  it('clamps ArrowUp at the first row', () => {
    expect(
      getFeatureWallRailNavigationTarget({ currentIndex: 0, key: 'ArrowUp', itemCount: 5 })
    ).toBe(0)
  })

  it('Home jumps to the first row', () => {
    expect(getFeatureWallRailNavigationTarget({ currentIndex: 4, key: 'Home', itemCount: 5 })).toBe(
      0
    )
  })

  it('End jumps to the last row', () => {
    expect(getFeatureWallRailNavigationTarget({ currentIndex: 0, key: 'End', itemCount: 5 })).toBe(
      4
    )
  })

  it('returns the current index for an empty list', () => {
    expect(
      getFeatureWallRailNavigationTarget({ currentIndex: 0, key: 'ArrowDown', itemCount: 0 })
    ).toBe(0)
  })

  it('returns the current index for an out-of-range starting position', () => {
    expect(
      getFeatureWallRailNavigationTarget({ currentIndex: 10, key: 'ArrowUp', itemCount: 5 })
    ).toBe(10)
  })
})
