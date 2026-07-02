import { describe, expect, it } from 'vitest'
import {
  buildDefaultTerminalOptions,
  DEFAULT_TERMINAL_FAST_SCROLL_SENSITIVITY,
  DEFAULT_TERMINAL_SCROLL_SENSITIVITY,
  normalizeTerminalFastScrollSensitivity,
  normalizeTerminalScrollSensitivity,
  resolveTerminalCursorInactiveStyle
} from './pane-terminal-options'
import { buildTerminalKeyboardProtocolOptions } from './terminal-keyboard-protocol'

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

  it('uses the shared desktop scrollback row default', () => {
    expect(buildDefaultTerminalOptions().scrollback).toBe(5_000)
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

  it('lets a local Windows ConPTY pane override the default and withhold kitty keyboard', () => {
    // Regression for #2434: per-pane options merge over the default the same way
    // createPaneDOM merges them, so a local Windows ConPTY override must win and
    // turn the advertised kittyKeyboard off (CSI-u-blind local CLIs ignore nav keys).
    const merged = {
      ...buildDefaultTerminalOptions(),
      ...buildTerminalKeyboardProtocolOptions({
        userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)',
        osRelease: '10.0.26100',
        connectionId: null,
        cwd: 'C:\\repo',
        shellOverride: 'powershell.exe',
        executionHostId: 'local'
      })
    }

    expect(merged.vtExtensions?.kittyKeyboard).toBe(false)
  })

  it('keeps the advertised kitty keyboard default for SSH and macOS/Linux panes', () => {
    for (const context of [
      {
        userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)',
        connectionId: 'ssh-1',
        cwd: 'C:\\repo',
        shellOverride: null,
        executionHostId: 'local'
      },
      {
        userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)',
        connectionId: null,
        cwd: '/repo',
        shellOverride: null,
        executionHostId: 'local'
      }
    ] as const) {
      const merged = {
        ...buildDefaultTerminalOptions(),
        ...buildTerminalKeyboardProtocolOptions(context)
      }

      expect(merged.vtExtensions?.kittyKeyboard).toBe(true)
    }
  })
})

// The xterm WebGL/ligatures attach tests were removed with the xterm DOM
// renderer: aterm owns GPU rendering (aterm-gpu-drawer) and shapes ligatures
// natively, so those render-addon paths no longer exist. The xterm
// Unicode-11-activation ordering test went the same way — aterm bakes Unicode 11
// width tables into the engine.
