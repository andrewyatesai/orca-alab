import { describe, expect, it, vi } from 'vitest'
import { hexToRgba, maybePushMode2031Flip, mode2031SequenceFor } from './terminal-appearance'

function fakeTransport(overrides?: { connected?: boolean; sendOk?: boolean }): {
  isConnected: () => boolean
  sendInput: ReturnType<typeof vi.fn<(data: string) => boolean>>
} {
  const connected = overrides?.connected ?? true
  const sendOk = overrides?.sendOk ?? true
  return {
    isConnected: () => connected,
    sendInput: vi.fn<(data: string) => boolean>(() => sendOk)
  }
}

describe('mode2031SequenceFor', () => {
  it('maps dark to CSI ?997;1n and light to CSI ?997;2n', () => {
    expect(mode2031SequenceFor('dark')).toBe('\x1b[?997;1n')
    expect(mode2031SequenceFor('light')).toBe('\x1b[?997;2n')
  })
})

describe('maybePushMode2031Flip', () => {
  it('does nothing when the pane has not subscribed to mode 2031', () => {
    const transport = fakeTransport()
    const subs = new Map<number, boolean>()
    const last = new Map<number, 'dark' | 'light'>()

    const pushed = maybePushMode2031Flip(1, 'dark', transport, subs, last)

    expect(pushed).toBe(false)
    expect(transport.sendInput).not.toHaveBeenCalled()
    expect(last.has(1)).toBe(false)
  })

  it('pushes the current mode once after subscribe and records it', () => {
    const transport = fakeTransport()
    const subs = new Map([[1, true]])
    const last = new Map<number, 'dark' | 'light'>()

    const pushed = maybePushMode2031Flip(1, 'dark', transport, subs, last)

    expect(pushed).toBe(true)
    expect(transport.sendInput).toHaveBeenCalledTimes(1)
    expect(transport.sendInput).toHaveBeenCalledWith('\x1b[?997;1n')
    expect(last.get(1)).toBe('dark')
  })

  it('suppresses repeat pushes when the resolved mode has not changed', () => {
    // This is the spam-gate: applyTerminalAppearance re-runs on every font /
    // opacity / cursor tweak, and we must not emit CSI 997 on each one.
    const transport = fakeTransport()
    const subs = new Map([[1, true]])
    const last = new Map<number, 'dark' | 'light'>()

    maybePushMode2031Flip(1, 'dark', transport, subs, last)
    maybePushMode2031Flip(1, 'dark', transport, subs, last)
    maybePushMode2031Flip(1, 'dark', transport, subs, last)

    expect(transport.sendInput).toHaveBeenCalledTimes(1)
    expect(last.get(1)).toBe('dark')
  })

  it('emits again when the theme actually flips', () => {
    const transport = fakeTransport()
    const subs = new Map([[1, true]])
    const last = new Map<number, 'dark' | 'light'>()

    maybePushMode2031Flip(1, 'dark', transport, subs, last)
    maybePushMode2031Flip(1, 'light', transport, subs, last)
    maybePushMode2031Flip(1, 'dark', transport, subs, last)

    expect(transport.sendInput.mock.calls.map((c) => c[0])).toEqual([
      '\x1b[?997;1n',
      '\x1b[?997;2n',
      '\x1b[?997;1n'
    ])
    expect(last.get(1)).toBe('dark')
  })

  it('does not push when the transport is disconnected', () => {
    const transport = fakeTransport({ connected: false })
    const subs = new Map([[1, true]])
    const last = new Map<number, 'dark' | 'light'>()

    const pushed = maybePushMode2031Flip(1, 'dark', transport, subs, last)

    expect(pushed).toBe(false)
    expect(transport.sendInput).not.toHaveBeenCalled()
    expect(last.has(1)).toBe(false)
  })

  it('leaves last-mode untouched when sendInput reports failure', () => {
    // So a reconnect / retry will re-emit on the next appearance pass.
    const transport = fakeTransport({ sendOk: false })
    const subs = new Map([[1, true]])
    const last = new Map<number, 'dark' | 'light'>()

    const pushed = maybePushMode2031Flip(1, 'dark', transport, subs, last)

    expect(pushed).toBe(false)
    expect(transport.sendInput).toHaveBeenCalledTimes(1)
    expect(last.has(1)).toBe(false)
  })

  it('tracks flip state per-pane', () => {
    const transportA = fakeTransport()
    const transportB = fakeTransport()
    const subs = new Map([
      [1, true],
      [2, true]
    ])
    const last = new Map<number, 'dark' | 'light'>()

    maybePushMode2031Flip(1, 'dark', transportA, subs, last)
    maybePushMode2031Flip(2, 'light', transportB, subs, last)
    maybePushMode2031Flip(1, 'dark', transportA, subs, last) // suppressed
    maybePushMode2031Flip(2, 'dark', transportB, subs, last) // flip

    expect(transportA.sendInput).toHaveBeenCalledTimes(1)
    expect(transportB.sendInput).toHaveBeenCalledTimes(2)
    expect(last.get(1)).toBe('dark')
    expect(last.get(2)).toBe('dark')
  })
})

// The renderer-side CSI mode-2031 handlers (installMode2031Handlers) are inert
// under aterm: the facade parser no-ops registerCsiHandler (see
// aterm-facade-parser.ts), so `CSI ?2031h/l` never reaches them. The real
// subscribe/reply behavior — recognizing mode 2031 and pushing the current
// color scheme — is handled natively by the aterm wasm engine and covered by
// the aterm e2e specs (tests/e2e/terminal-tab-switch-visual-restore.spec.ts via
// hiddenRendererMode2031ReplyCount), which run against the real engine + PTY.
// The pure spam-gate logic the renderer still owns (maybePushMode2031Flip) is
// covered above without an xterm parser.

describe('hexToRgba', () => {
  it('converts 6-char hex to rgba', () => {
    expect(hexToRgba('#1a1a1a', 0.72)).toBe('rgba(26, 26, 26, 0.72)')
  })

  it('converts 3-char shorthand hex to rgba', () => {
    expect(hexToRgba('#f0f', 0.5)).toBe('rgba(255, 0, 255, 0.5)')
  })

  it('handles full opacity', () => {
    expect(hexToRgba('#000000', 1)).toBe('rgba(0, 0, 0, 1)')
  })

  it('handles zero opacity', () => {
    expect(hexToRgba('#ffffff', 0)).toBe('rgba(255, 255, 255, 0)')
  })
})
