import { describe, expect, it } from 'vitest'
import {
  shouldClaimTerminalInterruptKeyboardEvent,
  shouldHandleTerminalInterruptKeyboardEvent,
  shouldSuppressTerminalInterruptKeyup,
  shouldSuppressTerminalModifierKeyboardEvent,
  TERMINAL_INTERRUPT_INPUT,
  TERMINAL_INTERRUPT_INPUT_KITTY,
  type XtermBypassEvent
} from './xterm-bypass-policy'

function event(overrides: Partial<XtermBypassEvent>): XtermBypassEvent {
  return {
    type: 'keydown',
    key: '',
    code: '',
    defaultPrevented: false,
    metaKey: false,
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    ...overrides
  }
}

describe('shouldHandleTerminalInterruptKeyboardEvent', () => {
  it('exports the ETX byte used for terminal interrupts', () => {
    expect(TERMINAL_INTERRUPT_INPUT).toBe('\x03')
  })

  it('handles macOS Ctrl+C as terminal interrupt even with a selection', () => {
    expect(
      shouldHandleTerminalInterruptKeyboardEvent(event({ key: 'c', code: 'KeyC', ctrlKey: true }), {
        isMac: true,
        hasSelection: true
      })
    ).toBe(true)
  })

  it('does not handle macOS Cmd+C so host copy can bypass xterm', () => {
    expect(
      shouldHandleTerminalInterruptKeyboardEvent(event({ key: 'c', code: 'KeyC', metaKey: true }), {
        isMac: true,
        hasSelection: true
      })
    ).toBe(false)
  })

  it('handles non-Mac Ctrl+C only when there is no selection', () => {
    expect(
      shouldHandleTerminalInterruptKeyboardEvent(event({ key: 'c', code: 'KeyC', ctrlKey: true }), {
        isMac: false,
        hasSelection: false
      })
    ).toBe(true)
    expect(
      shouldHandleTerminalInterruptKeyboardEvent(event({ key: 'c', code: 'KeyC', ctrlKey: true }), {
        isMac: false,
        hasSelection: true
      })
    ).toBe(false)
  })

  it('handles matching Ctrl+C keyup so kitty release sequences do not leak', () => {
    expect(
      shouldHandleTerminalInterruptKeyboardEvent(
        event({ type: 'keyup', key: 'c', code: 'KeyC', ctrlKey: true }),
        { isMac: false, hasSelection: false }
      )
    ).toBe(true)
  })

  it('suppresses handled Ctrl+C keyup even after Ctrl was released first', () => {
    expect(
      shouldSuppressTerminalInterruptKeyup(event({ type: 'keyup', key: 'c', code: 'KeyC' }))
    ).toBe(true)
    expect(
      shouldSuppressTerminalInterruptKeyup(
        event({ type: 'keyup', key: 'j', code: 'KeyC', keyCode: 67 })
      )
    ).toBe(false)
  })

  it('handles Ctrl+C by physical key metadata when the logical key is unavailable', () => {
    expect(
      shouldHandleTerminalInterruptKeyboardEvent(event({ key: '', code: 'KeyC', ctrlKey: true }), {
        isMac: false,
        hasSelection: false
      })
    ).toBe(true)
    expect(
      shouldHandleTerminalInterruptKeyboardEvent(
        event({ key: 'Unidentified', keyCode: 67, ctrlKey: true }),
        { isMac: true, hasSelection: false }
      )
    ).toBe(true)
  })

  it('does not handle physical KeyC when the logical key is a different letter', () => {
    expect(
      shouldHandleTerminalInterruptKeyboardEvent(event({ key: 'j', code: 'KeyC', ctrlKey: true }), {
        isMac: false,
        hasSelection: false
      })
    ).toBe(false)
  })

  it('does not handle modified Ctrl+C chords', () => {
    expect(
      shouldHandleTerminalInterruptKeyboardEvent(
        event({ key: 'C', code: 'KeyC', ctrlKey: true, shiftKey: true }),
        { isMac: false, hasSelection: false }
      )
    ).toBe(false)
    expect(
      shouldHandleTerminalInterruptKeyboardEvent(
        event({ key: 'c', code: 'KeyC', ctrlKey: true, altKey: true }),
        { isMac: true, hasSelection: false }
      )
    ).toBe(false)
  })
})

describe('shouldClaimTerminalInterruptKeyboardEvent (negotiation stand-down)', () => {
  it('exports the kitty CSI-u interrupt form for the intent detectors', () => {
    expect(TERMINAL_INTERRUPT_INPUT_KITTY).toBe('\x1b[99;5u')
  })

  it('claims plain Ctrl+C only when the app has NOT negotiated a key protocol', () => {
    const ctrlC = event({ key: 'c', code: 'KeyC', ctrlKey: true })
    expect(
      shouldClaimTerminalInterruptKeyboardEvent(ctrlC, {
        isMac: true,
        hasSelection: false,
        appKeyProtocolNegotiated: false
      })
    ).toBe(true)
    // Negotiated (kitty / modifyOtherKeys): stand down COMPLETELY — the engine
    // encoder emits the app's interrupt form (ESC[99;5u under kitty) from live
    // mode bits and the app keeps its flags across the interrupt.
    expect(
      shouldClaimTerminalInterruptKeyboardEvent(ctrlC, {
        isMac: true,
        hasSelection: false,
        appKeyProtocolNegotiated: true
      })
    ).toBe(false)
  })

  it('stands the keyup half down too (the engine release gating owns it)', () => {
    expect(
      shouldClaimTerminalInterruptKeyboardEvent(
        event({ type: 'keyup', key: 'c', code: 'KeyC', ctrlKey: true }),
        { isMac: false, hasSelection: false, appKeyProtocolNegotiated: true }
      )
    ).toBe(false)
  })

  it('keeps the un-negotiated platform rules intact (non-Mac selection copy)', () => {
    expect(
      shouldClaimTerminalInterruptKeyboardEvent(event({ key: 'c', code: 'KeyC', ctrlKey: true }), {
        isMac: false,
        hasSelection: true,
        appKeyProtocolNegotiated: false
      })
    ).toBe(false)
  })
})

describe('shouldSuppressTerminalModifierKeyboardEvent', () => {
  it('suppresses standalone modifier events before Kitty can encode them', () => {
    expect(
      shouldSuppressTerminalModifierKeyboardEvent(
        event({ type: 'keydown', key: 'Control', code: 'ControlLeft', ctrlKey: true })
      )
    ).toBe(true)
    expect(
      shouldSuppressTerminalModifierKeyboardEvent(
        event({ type: 'keyup', key: 'Meta', code: 'MetaLeft', metaKey: false })
      )
    ).toBe(true)
  })

  it('keeps suppressing under legacy / non-report-all mode bits', () => {
    // Kitty disambiguate + event types (Claude Code / Codex) still lack flag 8:
    // the engine encodes bare modifiers to nothing, so suppression stays on.
    expect(
      shouldSuppressTerminalModifierKeyboardEvent(
        event({ type: 'keydown', key: 'Shift', code: 'ShiftLeft', shiftKey: true }),
        { keyboardModeBits: 0x1 | 0x2 }
      )
    ).toBe(true)
    expect(
      shouldSuppressTerminalModifierKeyboardEvent(
        event({ type: 'keydown', key: 'Shift', code: 'ShiftLeft', shiftKey: true }),
        { keyboardModeBits: 0 }
      )
    ).toBe(true)
  })

  it('stands down under kitty REPORT_ALL_KEYS_AS_ESC so modifier reports reach the app', () => {
    // The engine now maps "Shift"/"Control"/… → ShiftLeft/… (Left-canonical)
    // and reports them under mode bit 0x100 — the host must let them through.
    expect(
      shouldSuppressTerminalModifierKeyboardEvent(
        event({ type: 'keydown', key: 'Shift', code: 'ShiftLeft', shiftKey: true }),
        { keyboardModeBits: 0x100 }
      )
    ).toBe(false)
    expect(
      shouldSuppressTerminalModifierKeyboardEvent(
        event({ type: 'keyup', key: 'Control', code: 'ControlLeft' }),
        { keyboardModeBits: 0x383 } // CSI = 31 u (all five kitty flags)
      )
    ).toBe(false)
  })

  it('does not suppress non-modifier keyboard input', () => {
    expect(
      shouldSuppressTerminalModifierKeyboardEvent(
        event({ type: 'keydown', key: 'c', code: 'KeyC', ctrlKey: true })
      )
    ).toBe(false)
    expect(shouldSuppressTerminalModifierKeyboardEvent(event({ type: 'keypress', key: 'c' }))).toBe(
      false
    )
    // Even under report-all, keypress events are not in scope.
    expect(
      shouldSuppressTerminalModifierKeyboardEvent(event({ type: 'keypress', key: 'Shift' }), {
        keyboardModeBits: 0x100
      })
    ).toBe(false)
  })
})
