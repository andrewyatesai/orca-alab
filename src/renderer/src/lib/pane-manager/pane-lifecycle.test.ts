import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { WebglAddon } from '@xterm/addon-webgl'
import type { ManagedPaneInternal } from './pane-manager-types'
import {
  attachWebgl,
  markComplexScriptOutput,
  resetTerminalWebglSuggestion
} from './pane-webgl-renderer'
import { attachLigatures } from './pane-ligatures'
import {
  buildDefaultTerminalOptions,
  DEFAULT_TERMINAL_FAST_SCROLL_SENSITIVITY,
  DEFAULT_TERMINAL_SCROLL_SENSITIVITY,
  normalizeTerminalFastScrollSensitivity,
  normalizeTerminalScrollSensitivity,
  resolveTerminalCursorInactiveStyle
} from './pane-terminal-options'

const webglMock = vi.hoisted(() => ({
  contextLossHandler: null as (() => void) | null,
  clearTextureAtlas: vi.fn(),
  dispose: vi.fn()
}))

// Why: these tests exercise the xterm rendering path (terminal.open + addon
// load order). The aterm renderer now ships default-on, so force it off here so
// openTerminal() takes the xterm branch under test.
vi.mock('./aterm/aterm-renderer-flag', () => ({
  isAtermRendererEnabled: () => false
}))

vi.mock('@xterm/addon-webgl', () => ({
  WebglAddon: vi.fn().mockImplementation(function WebglAddon() {
    return {
      onContextLoss: vi.fn((handler: () => void) => {
        webglMock.contextLossHandler = handler
      }),
      clearTextureAtlas: webglMock.clearTextureAtlas,
      dispose: webglMock.dispose
    }
  })
}))

function createPane(): ManagedPaneInternal {
  const leafId = '11111111-1111-4111-8111-111111111111' as never
  return {
    id: 1,
    leafId,
    stablePaneId: leafId,
    terminal: {
      loadAddon: vi.fn(),
      attachCustomWheelEventHandler: vi.fn(),
      refresh: vi.fn(),
      rows: 24
    } as never,
    container: {} as never,
    xtermContainer: {} as never,
    linkTooltip: {} as never,
    terminalGpuAcceleration: 'auto',
    gpuRenderingEnabled: true,
    webglAttachmentDeferred: false,
    webglDisabledAfterContextLoss: false,
    hasComplexScriptOutput: false,
    fitAddon: {
      fit: vi.fn()
    } as never,
    fitResizeObserver: null,
    pendingObservedFitRafId: null,
    searchAddon: {} as never,
    serializeAddon: {} as never,
    ligaturesAddon: null,
    webglAddon: null,
    pendingSplitScrollState: null,
    debugLabel: null
  }
}

describe('buildDefaultTerminalOptions', () => {
  it('leaves macOS Option available for keyboard layout characters', () => {
    expect(buildDefaultTerminalOptions().macOptionIsMeta).toBe(false)
  })

  it('uses the default inactive outline only for the block cursor', () => {
    expect(buildDefaultTerminalOptions().cursorStyle).toBe('block')
    expect(buildDefaultTerminalOptions().cursorInactiveStyle).toBe('outline')
  })

  it('shows the slim xterm scrollbar in its reserved gutter', () => {
    // Why: 7px gutter is an accepted ~1-column cost (VS Code reserves 14);
    // the v1.4.51 table corruption that once forced width 0 was the ZWJ
    // width bug, fixed separately by the Orca unicode provider.
    expect(buildDefaultTerminalOptions().scrollbar?.width).toBe(7)
  })

  it('slightly increases default terminal wheel scrolling while preserving fast scroll', () => {
    const options = buildDefaultTerminalOptions()

    expect(options.scrollSensitivity).toBe(DEFAULT_TERMINAL_SCROLL_SENSITIVITY)
    expect(options.fastScrollSensitivity).toBe(DEFAULT_TERMINAL_FAST_SCROLL_SENSITIVITY)
  })

  it('normalizes configurable terminal scroll sensitivity values', () => {
    expect(normalizeTerminalScrollSensitivity(undefined)).toBe(DEFAULT_TERMINAL_SCROLL_SENSITIVITY)
    expect(normalizeTerminalScrollSensitivity(0)).toBe(0.1)
    expect(normalizeTerminalScrollSensitivity(20)).toBe(10)
    expect(normalizeTerminalFastScrollSensitivity(undefined)).toBe(
      DEFAULT_TERMINAL_FAST_SCROLL_SENSITIVITY
    )
    expect(normalizeTerminalFastScrollSensitivity(0)).toBe(1)
    expect(normalizeTerminalFastScrollSensitivity(25)).toBe(20)
  })

  it('enables xterm contrast correction for low-contrast CLI colors', () => {
    expect(buildDefaultTerminalOptions().minimumContrastRatio).toBe(4.5)
  })

  it('only uses inactive outline for block cursors', () => {
    expect(resolveTerminalCursorInactiveStyle('block')).toBe('outline')
    expect(resolveTerminalCursorInactiveStyle('bar')).toBe('bar')
    expect(resolveTerminalCursorInactiveStyle('underline')).toBe('underline')
  })

  it('advertises kitty keyboard protocol so CLIs enable enhanced key reporting', () => {
    // Why: Orca already writes CSI-u bytes for extended key chords like
    // Shift+Enter on non-Windows platforms (see terminal-shortcut-policy.ts).
    // CLIs that gate enhanced input on a CSI ? u handshake only read those
    // bytes once the terminal advertises support. Regressing this flag
    // silently breaks enhanced chords, especially inside tmux.
    expect(buildDefaultTerminalOptions().vtExtensions?.kittyKeyboard).toBe(true)
  })
})

describe('attachWebgl', () => {
  beforeEach(() => {
    webglMock.contextLossHandler = null
    webglMock.clearTextureAtlas.mockClear()
    webglMock.dispose.mockClear()
    vi.mocked(WebglAddon).mockClear()
    resetTerminalWebglSuggestion()
    vi.stubGlobal('navigator', {
      platform: 'MacIntel',
      userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)'
    })
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
      callback(16)
      return 1
    })
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('keeps a pane on the DOM renderer after WebGL context loss', () => {
    const pane = createPane()
    pane.terminalGpuAcceleration = 'on'

    attachWebgl(pane)
    expect(pane.terminal.loadAddon).toHaveBeenCalledTimes(1)
    expect(webglMock.contextLossHandler).not.toBeNull()
    vi.mocked(pane.terminal.refresh).mockClear()

    webglMock.contextLossHandler?.()

    expect(pane.webglAddon).toBeNull()
    expect(pane.webglDisabledAfterContextLoss).toBe(true)
    expect(pane.fitAddon.fit).toHaveBeenCalledTimes(1)
    expect(pane.terminal.refresh).toHaveBeenCalledWith(0, 23)

    attachWebgl(pane)

    expect(pane.terminal.loadAddon).toHaveBeenCalledTimes(1)
  })

  it('repaints the current buffer after WebGL attaches', () => {
    const pane = createPane()
    pane.terminalGpuAcceleration = 'on'

    attachWebgl(pane)

    expect(pane.terminal.refresh).toHaveBeenCalledWith(0, 23)
  })

  it('clears the WebGL texture atlas and refreshes the buffer on recovery', async () => {
    const { resetWebglTextureAtlas } = await import('./pane-webgl-renderer')
    const pane = createPane()
    pane.terminalGpuAcceleration = 'on'

    attachWebgl(pane)
    vi.mocked(pane.terminal.refresh).mockClear()
    resetWebglTextureAtlas(pane)

    expect(webglMock.clearTextureAtlas).toHaveBeenCalledTimes(1)
    expect(pane.terminal.refresh).toHaveBeenCalledWith(0, 23)
  })

  it('does not reset a WebGL atlas after context-loss fallback', async () => {
    const { resetWebglTextureAtlas } = await import('./pane-webgl-renderer')
    const pane = createPane()
    pane.terminalGpuAcceleration = 'on'

    attachWebgl(pane)
    webglMock.contextLossHandler?.()
    vi.mocked(pane.terminal.refresh).mockClear()
    webglMock.clearTextureAtlas.mockClear()
    resetWebglTextureAtlas(pane)

    expect(webglMock.clearTextureAtlas).not.toHaveBeenCalled()
    expect(pane.terminal.refresh).not.toHaveBeenCalled()
  })

  it('does not attach WebGL while initial rendering is deferred', () => {
    const pane = createPane()
    pane.terminalGpuAcceleration = 'on'
    pane.webglAttachmentDeferred = true

    attachWebgl(pane)

    expect(pane.webglAddon).toBeNull()
    expect(pane.terminal.loadAddon).not.toHaveBeenCalled()
  })

  it('does not attach WebGL when terminal GPU acceleration is off', () => {
    const pane = createPane()
    pane.terminalGpuAcceleration = 'off'

    attachWebgl(pane)

    expect(pane.webglAddon).toBeNull()
    expect(pane.terminal.loadAddon).not.toHaveBeenCalled()
  })

  it('uses WebGL rendering for auto GPU acceleration on non-Linux platforms', () => {
    const pane = createPane()

    attachWebgl(pane)

    expect(pane.webglAddon).not.toBeNull()
    expect(pane.terminal.loadAddon).toHaveBeenCalledTimes(1)
  })

  it('uses DOM rendering for auto GPU acceleration on Linux', () => {
    vi.stubGlobal('navigator', {
      platform: 'Linux x86_64',
      userAgent: 'Mozilla/5.0 (X11; Linux x86_64)'
    })
    const pane = createPane()

    attachWebgl(pane)

    expect(pane.webglAddon).toBeNull()
    expect(pane.terminal.loadAddon).not.toHaveBeenCalled()
  })

  it('uses WebGL rendering for Linux auto GPU acceleration on hardware renderers', () => {
    const rendererKey = 0x9246
    const vendorKey = 0x9245
    vi.stubGlobal('navigator', {
      platform: 'Linux x86_64',
      userAgent: 'Mozilla/5.0 (X11; Linux x86_64)'
    })
    vi.stubGlobal('document', {
      createElement: vi.fn((tagName: string) => {
        if (tagName !== 'canvas') {
          return {}
        }
        return {
          getContext: vi.fn((contextName: string) =>
            contextName === 'webgl2'
              ? {
                  getExtension: vi.fn(() => ({
                    UNMASKED_RENDERER_WEBGL: rendererKey,
                    UNMASKED_VENDOR_WEBGL: vendorKey
                  })),
                  getParameter: vi.fn((key: number) =>
                    key === rendererKey
                      ? 'Mesa Intel(R) UHD Graphics 770'
                      : key === vendorKey
                        ? 'Intel'
                        : null
                  )
                }
              : null
          )
        }
      })
    })
    resetTerminalWebglSuggestion()
    const pane = createPane()

    attachWebgl(pane)

    expect(pane.webglAddon).not.toBeNull()
    expect(pane.terminal.loadAddon).toHaveBeenCalledTimes(1)
  })

  it('still allows forced WebGL on Linux', () => {
    vi.stubGlobal('navigator', {
      platform: 'Linux x86_64',
      userAgent: 'Mozilla/5.0 (X11; Linux x86_64)'
    })
    const pane = createPane()
    pane.terminalGpuAcceleration = 'on'

    attachWebgl(pane)

    expect(pane.terminal.loadAddon).toHaveBeenCalledTimes(1)
  })

  it('keeps auto-mode panes on WebGL after complex-script output', () => {
    const pane = createPane()

    attachWebgl(pane)
    expect(pane.terminal.loadAddon).toHaveBeenCalledTimes(1)
    vi.mocked(pane.terminal.loadAddon).mockClear()

    markComplexScriptOutput(pane)

    expect(pane.hasComplexScriptOutput).toBe(true)
    expect(pane.webglAddon).not.toBeNull()
    expect(webglMock.dispose).not.toHaveBeenCalled()
    expect(pane.fitAddon.fit).not.toHaveBeenCalled()

    attachWebgl(pane)

    expect(pane.terminal.loadAddon).toHaveBeenCalledTimes(1)
  })

  it('keeps later auto panes on DOM after WebGL attach fails', () => {
    vi.mocked(WebglAddon).mockImplementationOnce(() => {
      throw new Error('webgl unavailable')
    })
    const firstPane = createPane()
    const secondPane = createPane()

    attachWebgl(firstPane)
    attachWebgl(secondPane)

    expect(firstPane.webglAddon).toBeNull()
    expect(secondPane.webglAddon).toBeNull()
    expect(secondPane.terminal.loadAddon).not.toHaveBeenCalled()
  })

  it('keeps forced WebGL on after complex-script output', () => {
    const pane = createPane()

    markComplexScriptOutput(pane)
    pane.terminalGpuAcceleration = 'on'
    attachWebgl(pane)

    expect(pane.terminal.loadAddon).toHaveBeenCalledTimes(1)
  })
})

describe('attachLigatures', () => {
  it('refreshes existing rows after loading the ligatures addon', () => {
    const pane = createPane()

    attachLigatures(pane)

    expect(pane.terminal.loadAddon).toHaveBeenCalledTimes(1)
    expect(pane.terminal.refresh).toHaveBeenCalledWith(0, 23)
    expect(pane.ligaturesAddon).not.toBeNull()
  })
})

// The xterm Unicode-11-activation ordering test was removed with
// openXtermRenderer: aterm bakes Unicode 11 width tables into the engine, so
// there is no xterm buffer to activate a width provider on before the first write.
