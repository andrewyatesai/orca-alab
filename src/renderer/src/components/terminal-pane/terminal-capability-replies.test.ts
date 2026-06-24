import { describe, expect, it, vi } from 'vitest'
import { Terminal } from '@xterm/headless'
import {
  CONPTY_DA1_RESPONSE,
  DA1_RESPONSE_WITH_SIXEL,
  DEFAULT_DA1_RESPONSE,
  createTerminalOscColorQueryResponder,
  createTerminalPixelSizeQueryResponder,
  installTerminalCapabilityReplyHandlers
} from './terminal-capability-replies'

function writeTerminal(term: Terminal, data: string): Promise<void> {
  return new Promise((resolve) => term.write(data, resolve))
}

function createElement(width: number, height: number): HTMLElement {
  return {
    querySelector: () => ({
      getBoundingClientRect: () => ({ width, height })
    })
  } as unknown as HTMLElement
}

describe('installTerminalCapabilityReplyHandlers', () => {
  it('answers primary DA1 with the default xterm-compatible response', async () => {
    const term = new Terminal({ cols: 80, rows: 24, allowProposedApi: true })
    const sendInput = vi.fn<(data: string) => boolean>(() => true)
    const disposable = installTerminalCapabilityReplyHandlers({
      terminal: term as never,
      parser: term.parser,
      sendInput,
      isReplaying: () => false
    })

    try {
      await writeTerminal(term, '\x1b[c')

      expect(sendInput).toHaveBeenCalledTimes(1)
      expect(sendInput).toHaveBeenCalledWith(DEFAULT_DA1_RESPONSE)
    } finally {
      disposable.dispose()
      term.dispose()
    }
  })

  it('resolves a da1Response getter live (aterm panes advertise Sixel)', async () => {
    const term = new Terminal({ cols: 80, rows: 24, allowProposedApi: true })
    const sendInput = vi.fn<(data: string) => boolean>(() => true)
    let atermActive = false
    const disposable = installTerminalCapabilityReplyHandlers({
      terminal: term as never,
      parser: term.parser,
      sendInput,
      isReplaying: () => false,
      // Getter form: the renderer-authoritative DA1 depends on live pane state.
      da1Response: () => (atermActive ? DA1_RESPONSE_WITH_SIXEL : DEFAULT_DA1_RESPONSE)
    })

    try {
      await writeTerminal(term, '\x1b[c')
      expect(sendInput).toHaveBeenLastCalledWith(DEFAULT_DA1_RESPONSE)
      // The Sixel DA1 carries param 4 (the bit apps gate Sixel support on).
      expect(DA1_RESPONSE_WITH_SIXEL).toContain(';4c')

      atermActive = true
      await writeTerminal(term, '\x1b[c')
      expect(sendInput).toHaveBeenLastCalledWith(DA1_RESPONSE_WITH_SIXEL)
    } finally {
      disposable.dispose()
      term.dispose()
    }
  })

  it('keeps the ConPTY basic conformance response override', async () => {
    const term = new Terminal({ cols: 80, rows: 24, allowProposedApi: true })
    const sendInput = vi.fn<(data: string) => boolean>(() => true)
    const disposable = installTerminalCapabilityReplyHandlers({
      terminal: term as never,
      parser: term.parser,
      sendInput,
      isReplaying: () => false,
      da1Response: CONPTY_DA1_RESPONSE
    })

    try {
      await writeTerminal(term, '\x1b[c')

      expect(sendInput).toHaveBeenCalledWith(CONPTY_DA1_RESPONSE)
    } finally {
      disposable.dispose()
      term.dispose()
    }
  })

  it('suppresses xterm XTVERSION + ANSI-DECRQM auto-replies for aterm-owned panes', async () => {
    // aterm drains its OWN XTVERSION/DECRQM; the kept xterm shim must NOT also
    // auto-reply (it would leak a second "xterm.js(...)" / "$y" into the shell).
    const term = new Terminal({ cols: 80, rows: 24, allowProposedApi: true })
    const onData = vi.fn<(data: string) => void>()
    term.onData(onData)
    let atermOwned = true
    const disposable = installTerminalCapabilityReplyHandlers({
      terminal: term as never,
      parser: term.parser,
      sendInput: vi.fn(),
      isReplaying: () => false,
      isAtermReplyOwned: () => atermOwned
    })

    try {
      // aterm-owned: xterm's own XTVERSION (CSI > q) + ANSI DECRQM (CSI 4 $ p)
      // replies are consumed, so onData never fires.
      await writeTerminal(term, '\x1b[>q')
      await writeTerminal(term, '\x1b[4$p')
      expect(onData, 'xterm must not auto-reply while aterm owns replies').not.toHaveBeenCalled()

      // Non-aterm (xterm fallback): xterm answers XTVERSION itself again.
      atermOwned = false
      await writeTerminal(term, '\x1b[>q')
      expect(onData, 'xterm answers XTVERSION on the non-aterm path').toHaveBeenCalled()
    } finally {
      disposable.dispose()
      term.dispose()
    }
  })

  it('suppresses xterm DECRQSS (DCS $q) + kitty ?u auto-replies for aterm-owned panes', async () => {
    // aterm drains its OWN DECRQSS + kitty-keyboard-query replies; the kept xterm
    // shim must NOT also auto-reply (it would leak a second "DCS 1$r...ST" / "[?Nu"
    // into the shell). These live on the DCS surface (DECRQSS) and the ?u CSI — the
    // exact double-answer class the CSI XTVERSION suppressor doesn't reach.
    const term = new Terminal({ cols: 80, rows: 24, allowProposedApi: true })
    // Kitty query only fires when the extension is enabled (mirrors production).
    term.options.vtExtensions = { kittyKeyboard: true }
    const onData = vi.fn<(data: string) => void>()
    term.onData(onData)
    let atermOwned = true
    const disposable = installTerminalCapabilityReplyHandlers({
      terminal: term as never,
      parser: term.parser,
      sendInput: vi.fn(),
      isReplaying: () => false,
      isAtermReplyOwned: () => atermOwned
    })

    try {
      // aterm-owned: xterm's DECRQSS (DCS $ q "q" ST → DECSCUSR) + kitty query (CSI ? u)
      // are consumed, so onData never fires.
      await writeTerminal(term, '\x1bP$q q\x1b\\')
      await writeTerminal(term, '\x1b[?u')
      expect(onData, 'xterm must not auto-reply DECRQSS/kitty while aterm owns replies').not.toHaveBeenCalled()

      // Non-aterm (xterm fallback): xterm answers the kitty query itself again.
      atermOwned = false
      await writeTerminal(term, '\x1b[?u')
      expect(onData, 'xterm answers the kitty query on the non-aterm path').toHaveBeenCalled()
    } finally {
      disposable.dispose()
      term.dispose()
    }
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
      const observe = createTerminalOscColorQueryResponder(sendInput, () => null, () => false)

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

  it('consumes replayed capability queries without sending input to the shell', async () => {
    const term = new Terminal({ cols: 80, rows: 24, allowProposedApi: true })
    const sendInput = vi.fn<(data: string) => boolean>(() => true)
    const disposable = installTerminalCapabilityReplyHandlers({
      terminal: { ...term, element: createElement(800, 480) } as never,
      parser: term.parser,
      sendInput,
      isReplaying: () => true
    })

    try {
      await writeTerminal(term, '\x1b[0c')

      expect(sendInput).not.toHaveBeenCalled()
    } finally {
      disposable.dispose()
      term.dispose()
    }
  })

  it('leaves non-primary DA queries to other handlers', async () => {
    const term = new Terminal({ cols: 80, rows: 24, allowProposedApi: true })
    const sendInput = vi.fn<(data: string) => boolean>(() => true)
    const returnValues: boolean[] = []
    const disposable = installTerminalCapabilityReplyHandlers({
      terminal: term as never,
      parser: {
        registerCsiHandler: (id, cb) =>
          term.parser.registerCsiHandler(id, (params) => {
            const value = cb(params) as boolean
            returnValues.push(value)
            return value
          }),
        registerDcsHandler: (id, cb) =>
          term.parser.registerDcsHandler(id, (data, params) => cb(data, params) as boolean)
      },
      sendInput,
      isReplaying: () => false
    })

    try {
      await writeTerminal(term, '\x1b[1c')

      expect(sendInput).not.toHaveBeenCalled()
      expect(returnValues).toEqual([false])
    } finally {
      disposable.dispose()
      term.dispose()
    }
  })
})
