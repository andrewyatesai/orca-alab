import { describe, expect, it } from 'vitest'
import {
  createComposeBoxImeEnterGuard,
  TERMINAL_IME_ENTER_REDISPATCH_ABSORB_WINDOW_MS
} from './terminal-compose-box-ime-guard'

describe('createComposeBoxImeEnterGuard', () => {
  it('absorbs Enter during composition and keyCode 229', () => {
    const guard = createComposeBoxImeEnterGuard(() => 0)

    guard.onCompositionStart()
    // Composition tracked locally: even an event reporting isComposing=false mid-preedit is absorbed.
    expect(guard.shouldAbsorbEnter({ isComposing: false, keyCode: 13 })).toBe(true)
    expect(guard.shouldAbsorbEnter({ isComposing: true, keyCode: 13 })).toBe(true)
    expect(guard.shouldAbsorbEnter({ isComposing: false, keyCode: 229 })).toBe(true)
  })

  it('absorbs the macOS Hangul re-dispatched Enter inside the 50ms window', () => {
    let now = 1_000
    const guard = createComposeBoxImeEnterGuard(() => now)

    guard.onCompositionStart()
    guard.onCompositionEnd()
    now += TERMINAL_IME_ENTER_REDISPATCH_ABSORB_WINDOW_MS - 1
    // The re-dispatch arrives as a plain keydown ~2ms after compositionend.
    expect(guard.shouldAbsorbEnter({ isComposing: false, keyCode: 13 })).toBe(true)
    // Exactly one credit: the very next plain Enter is real.
    expect(guard.shouldAbsorbEnter({ isComposing: false, keyCode: 13 })).toBe(false)
  })

  it('submits a real Enter after the absorb window expires', () => {
    let now = 1_000
    const guard = createComposeBoxImeEnterGuard(() => now)

    guard.onCompositionStart()
    guard.onCompositionEnd()
    now += TERMINAL_IME_ENTER_REDISPATCH_ABSORB_WINDOW_MS + 1
    expect(guard.shouldAbsorbEnter({ isComposing: false, keyCode: 13 })).toBe(false)
  })
})
