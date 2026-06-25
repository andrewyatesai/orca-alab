import { describe, expect, it } from 'vitest'
import {
  buildDefaultTerminalOptions,
  DEFAULT_TERMINAL_FAST_SCROLL_SENSITIVITY,
  DEFAULT_TERMINAL_SCROLL_SENSITIVITY,
  normalizeTerminalFastScrollSensitivity,
  normalizeTerminalScrollSensitivity,
  resolveTerminalCursorInactiveStyle
} from './pane-terminal-options'

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

// The xterm WebGL/ligatures attach tests were removed with the xterm DOM
// renderer: aterm owns GPU rendering (aterm-gpu-drawer) and shapes ligatures
// natively, so those render-addon paths no longer exist. The xterm
// Unicode-11-activation ordering test went the same way — aterm bakes Unicode 11
// width tables into the engine.
