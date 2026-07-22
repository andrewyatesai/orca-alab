// @vitest-environment happy-dom

import { act, renderHook } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { useFeatureWallTourKeyboardShortcut } from './use-feature-wall-tour-keyboard-shortcut'

function submitEvent(repeat: boolean): KeyboardEvent {
  return new KeyboardEvent('keydown', {
    key: 'Enter',
    ctrlKey: true,
    repeat,
    bubbles: true,
    cancelable: true
  })
}

describe('useFeatureWallTourKeyboardShortcut', () => {
  it('advances once for an initial shortcut press and ignores key repeat', () => {
    const onContinue = vi.fn()
    renderHook(() =>
      useFeatureWallTourKeyboardShortcut({ isOpen: true, enabled: true, onContinue })
    )

    const initial = submitEvent(false)
    const repeated = submitEvent(true)
    act(() => {
      window.dispatchEvent(initial)
      window.dispatchEvent(repeated)
    })

    expect(onContinue).toHaveBeenCalledTimes(1)
    expect(initial.defaultPrevented).toBe(true)
    expect(repeated.defaultPrevented).toBe(false)
  })
})
