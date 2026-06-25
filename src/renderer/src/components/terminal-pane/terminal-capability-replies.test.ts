import { describe, expect, it, vi } from 'vitest'
import {
  CONPTY_DA1_RESPONSE,
  DA1_RESPONSE_WITH_SIXEL,
  DEFAULT_DA1_RESPONSE,
  createTerminalOscColorQueryResponder,
  createTerminalPixelSizeQueryResponder
} from './terminal-capability-replies'

function createElement(width: number, height: number): HTMLElement {
  return {
    querySelector: () => ({
      getBoundingClientRect: () => ({ width, height })
    })
  } as unknown as HTMLElement
}

describe('installTerminalCapabilityReplyHandlers', () => {
  // The CSI/DCS reply handlers (DA1 default/getter/ConPTY, XTVERSION/DECRQM/
  // DECRQSS/kitty suppression, replayed-query consumption, non-primary DA
  // passthrough) are inert under aterm: the facade parser no-ops
  // registerCsiHandler/registerDcsHandler (see aterm-facade-parser.ts), so these
  // sequences never reach the renderer handlers. The aterm wasm engine answers
  // DA1/DA2/DSR/CPR/DECRQM/CSI-14t/16t/OSC-10/11 natively via take_response; that
  // real path is covered end-to-end against the engine + PTY by
  // tests/e2e/aterm-query-replies.spec.ts.
  //
  // The DA1 response constants are still authored renderer-side, so guard their
  // literal bytes directly (pure string check, no xterm parser).
  it('exposes the expected DA1 response constants', () => {
    expect(DEFAULT_DA1_RESPONSE).toBe('\x1b[?1;2c')
    expect(CONPTY_DA1_RESPONSE).toBe('\x1b[?61;4c')
    // The Sixel DA1 carries param 4 (the bit apps gate Sixel support on).
    expect(DA1_RESPONSE_WITH_SIXEL).toBe('\x1b[?1;2;4c')
    expect(DA1_RESPONSE_WITH_SIXEL).toContain(';4c')
  })

  it('answers window and cell pixel-size reports from renderer geometry', () => {
    const sendInput = vi.fn<(data: string) => boolean>(() => true)
    const observe = createTerminalPixelSizeQueryResponder(
      {
        cols: 100,
        rows: 40,
        element: createElement(900, 720)
      },
      sendInput
    )

    observe('\x1b[14t\x1b[16t')

    expect(sendInput).toHaveBeenCalledWith('\x1b[4;720;900t')
    expect(sendInput).toHaveBeenCalledWith('\x1b[6;18;9t')
  })

  it('prefers the aterm renderer pixel size over the xterm DOM measurement', () => {
    // Why: an aterm pane's xterm is unopened (no .xterm-screen). The canvas
    // controller is authoritative for pixel size, so 14t/16t must answer from
    // its framebuffer + cell device px, not the (absent/zero) DOM rect.
    const sendInput = vi.fn<(data: string) => boolean>(() => true)
    const observe = createTerminalPixelSizeQueryResponder(
      // element measurement would yield different numbers; renderer source wins.
      { cols: 100, rows: 40, element: createElement(900, 720) },
      sendInput,
      () => ({ width: 1600, height: 960, cellWidth: 16, cellHeight: 32 })
    )

    observe('\x1b[14t\x1b[16t')

    // 14t -> CSI 4 ; heightPx ; widthPx t (text-area framebuffer device px)
    expect(sendInput).toHaveBeenCalledWith('\x1b[4;960;1600t')
    // 16t -> CSI 6 ; cellHpx ; cellWpx t
    expect(sendInput).toHaveBeenCalledWith('\x1b[6;32;16t')
  })

  it('falls back to the xterm DOM measurement when the renderer source returns null', () => {
    const sendInput = vi.fn<(data: string) => boolean>(() => true)
    const observe = createTerminalPixelSizeQueryResponder(
      { cols: 100, rows: 40, element: createElement(900, 720) },
      sendInput,
      () => null
    )

    observe('\x1b[14t\x1b[16t')

    expect(sendInput).toHaveBeenCalledWith('\x1b[4;720;900t')
    expect(sendInput).toHaveBeenCalledWith('\x1b[6;18;9t')
  })

  it('skips the reply when the renderer reports a zero-sized framebuffer', () => {
    // Why: before the first render the framebuffer can be 0×0; don't emit a
    // bogus "\x1b[4;0;0t" — wait until there's a real size.
    const sendInput = vi.fn<(data: string) => boolean>(() => true)
    const observe = createTerminalPixelSizeQueryResponder(
      { cols: 100, rows: 40, element: createElement(900, 720) },
      sendInput,
      () => ({ width: 0, height: 0, cellWidth: 0, cellHeight: 0 })
    )

    observe('\x1b[14t')

    expect(sendInput).not.toHaveBeenCalled()
  })

  it('answers split pixel-size reports', () => {
    const sendInput = vi.fn<(data: string) => boolean>(() => true)
    const observe = createTerminalPixelSizeQueryResponder(
      {
        cols: 100,
        rows: 40,
        element: createElement(900, 720)
      },
      sendInput
    )

    observe('\x1b[')
    observe('16t')

    expect(sendInput).toHaveBeenCalledWith('\x1b[6;18;9t')
  })

  describe('createTerminalOscColorQueryResponder', () => {
    it('answers OSC 11 background with the aterm theme bg as 16-bit rgb', () => {
      const sendInput = vi.fn<(data: string) => boolean>(() => true)
      const observe = createTerminalOscColorQueryResponder(
        sendInput,
        () => ({ fg: 0xd0d0d0, bg: 0x111318 }),
        () => false
      )

      observe('\x1b]11;?\x07')

      expect(sendInput).toHaveBeenCalledWith('\x1b]11;rgb:1111/1313/1818\x07')
    })

    it('answers OSC 10 foreground', () => {
      const sendInput = vi.fn<(data: string) => boolean>(() => true)
      const observe = createTerminalOscColorQueryResponder(
        sendInput,
        () => ({ fg: 0xabcdef, bg: 0x000000 }),
        () => false
      )

      observe('\x1b]10;?\x07')

      expect(sendInput).toHaveBeenCalledWith('\x1b]10;rgb:abab/cdcd/efef\x07')
    })

    it('does not reply when not aterm-rendered (no theme source)', () => {
      const sendInput = vi.fn<(data: string) => boolean>(() => true)
      const observe = createTerminalOscColorQueryResponder(
        sendInput,
        () => null,
        () => false
      )

      observe('\x1b]11;?\x07')

      expect(sendInput).not.toHaveBeenCalled()
    })

    it('does not reply to replayed OSC color queries (stray-input guard)', () => {
      const sendInput = vi.fn<(data: string) => boolean>(() => true)
      const observe = createTerminalOscColorQueryResponder(
        sendInput,
        () => ({ fg: 0xffffff, bg: 0x000000 }),
        () => true
      )

      observe('\x1b]11;?\x07')

      expect(sendInput).not.toHaveBeenCalled()
    })

    it('answers a query split across chunks exactly once', () => {
      const sendInput = vi.fn<(data: string) => boolean>(() => true)
      const observe = createTerminalOscColorQueryResponder(
        sendInput,
        () => ({ fg: 0xffffff, bg: 0x222222 }),
        () => false
      )

      observe('\x1b]11')
      observe(';?\x07')

      expect(sendInput).toHaveBeenCalledTimes(1)
      expect(sendInput).toHaveBeenCalledWith('\x1b]11;rgb:2222/2222/2222\x07')
    })

    it('does not double-reply across consecutive chunks after a full query', () => {
      const sendInput = vi.fn<(data: string) => boolean>(() => true)
      const observe = createTerminalOscColorQueryResponder(
        sendInput,
        () => ({ fg: 0xffffff, bg: 0x222222 }),
        () => false
      )

      observe('\x1b]11;?\x07')
      observe('some later output\r\n')

      expect(sendInput).toHaveBeenCalledTimes(1)
    })
  })
})
