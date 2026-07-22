import { describe, expect, it } from 'vitest'
import type { ResolvedCustomKeybinding } from '../../../../shared/custom-keybindings'
import {
  resolveTerminalShortcutAction,
  type TerminalShortcutEvent
} from './terminal-shortcut-policy'

function event(overrides: Partial<TerminalShortcutEvent>): TerminalShortcutEvent {
  return {
    key: '',
    code: '',
    metaKey: false,
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    repeat: false,
    ...overrides
  }
}

describe('resolveTerminalShortcutAction — custom keybindings', () => {
  const shiftEnterMacro: ResolvedCustomKeybinding = {
    id: 'custom.shiftenter01',
    title: 'Kitty Shift+Enter',
    action: { type: 'sendText', text: '\\x1b[13;2u' },
    bindings: ['Shift+Enter'],
    decodedText: '\x1bOM'
  }
  const bareRemap: ResolvedCustomKeybinding = {
    id: 'custom.bareperiod01',
    title: 'ASCII period',
    action: { type: 'sendText', text: '.' },
    bindings: ['Period'],
    matchPhysicalKey: true,
    decodedText: '.'
  }
  const quickCommand: ResolvedCustomKeybinding = {
    id: 'custom.quickcmd0001',
    title: 'Run rebuild',
    action: { type: 'runQuickCommand', quickCommandId: 'qc-rebuild' },
    bindings: ['Mod+Alt+B']
  }

  function resolveWithCustom(
    input: TerminalShortcutEvent,
    custom: readonly ResolvedCustomKeybinding[],
    keybindings?: Parameters<typeof resolveTerminalShortcutAction>[5]
  ): ReturnType<typeof resolveTerminalShortcutAction> {
    return resolveTerminalShortcutAction(
      input,
      true,
      'false',
      0,
      false,
      keybindings,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      custom
    )
  }

  it('custom sendText on Shift+Enter beats the hardcoded \\x1b[13;2u rewrite', () => {
    // Shift-only chords also suppress companion insertion (no non-Shift modifiers).
    expect(
      resolveWithCustom(event({ key: 'Enter', code: 'Enter', shiftKey: true }), [shiftEnterMacro])
    ).toEqual({ type: 'sendInput', data: '\x1bOM', suppressTextInsertion: true })
    // Without the custom entry the built-in rewrite still applies.
    expect(
      resolveWithCustom(event({ key: 'Enter', code: 'Enter', shiftKey: true }), [])
    ).toEqual({ type: 'sendInput', data: '\x1b\r' })
  })

  it('built-in terminal.copySelection chord beats a same-chord custom entry', () => {
    const clashing: ResolvedCustomKeybinding = {
      ...bareRemap,
      bindings: ['Mod+Shift+C'],
      matchPhysicalKey: false
    }
    expect(
      resolveWithCustom(
        event({ key: 'c', code: 'KeyC', metaKey: true, shiftKey: true }),
        [clashing]
      )
    ).toEqual({ type: 'copySelection' })
  })

  it('a held built-in chord does not fire a same-chord custom sendText on repeats', () => {
    const clashing: ResolvedCustomKeybinding = {
      ...bareRemap,
      bindings: ['Mod+Shift+C'],
      matchPhysicalKey: false
    }
    // Repeats skip the !repeat built-in ladder; the custom branch must not pick the chord up.
    expect(
      resolveWithCustom(
        event({ key: 'c', code: 'KeyC', metaKey: true, shiftKey: true, repeat: true }),
        [clashing]
      )
    ).toBeNull()
    // A disabled built-in frees the chord for the custom entry, even on repeat.
    expect(
      resolveWithCustom(
        event({ key: 'c', code: 'KeyC', metaKey: true, shiftKey: true, repeat: true }),
        [clashing],
        { 'terminal.copySelection': [] }
      )
    ).toMatchObject({ type: 'sendInput', data: '.' })
  })

  it('custom sendText fires on event.repeat; runQuickCommand does not', () => {
    expect(
      resolveWithCustom(
        event({ key: 'Enter', code: 'Enter', shiftKey: true, repeat: true }),
        [shiftEnterMacro]
      )
    ).toMatchObject({ type: 'sendInput', data: '\x1bOM' })
    expect(
      resolveWithCustom(event({ key: 'b', code: 'KeyB', metaKey: true, altKey: true }), [
        quickCommand
      ])
    ).toEqual({ type: 'runQuickCommand', quickCommandId: 'qc-rebuild' })
    expect(
      resolveWithCustom(
        event({ key: 'b', code: 'KeyB', metaKey: true, altKey: true, repeat: true }),
        [quickCommand]
      )
    ).toEqual({ type: 'consumeKey' })
  })

  it('isComposing: true suppresses the custom match and falls through unchanged', () => {
    expect(
      resolveWithCustom(
        event({ key: 'Enter', code: 'Enter', shiftKey: true, isComposing: true }),
        [shiftEnterMacro]
      )
    ).toEqual({ type: 'sendInput', data: '\x1b\r' })
    expect(
      resolveWithCustom(event({ key: 'Process', code: 'Period' }), [bareRemap])
    ).toBeNull()
    expect(
      resolveWithCustom(event({ key: '。', code: 'Period', isComposing: true }), [bareRemap])
    ).toBeNull()
  })

  it('bare chord sets suppressTextInsertion; modified chord does not', () => {
    expect(resolveWithCustom(event({ key: '.', code: 'Period' }), [bareRemap])).toEqual({
      type: 'sendInput',
      data: '.',
      suppressTextInsertion: true
    })
    const modified: ResolvedCustomKeybinding = {
      ...shiftEnterMacro,
      bindings: ['Mod+Alt+P']
    }
    expect(
      resolveWithCustom(event({ key: 'p', code: 'KeyP', metaKey: true, altKey: true }), [modified])
    ).toMatchObject({ suppressTextInsertion: false })
  })

  it('matches the composed full-width char by physical code (#9338)', () => {
    expect(resolveWithCustom(event({ key: '。', code: 'Period' }), [bareRemap])).toEqual({
      type: 'sendInput',
      data: '.',
      suppressTextInsertion: true
    })
  })
})
