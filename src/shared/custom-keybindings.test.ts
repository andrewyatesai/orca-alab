import { describe, expect, it } from 'vitest'
import {
  decodeCustomSendText,
  generateCustomKeybindingId,
  isCustomKeybindingId,
  matchCustomKeybinding,
  resolveKeybindingTitle,
  CUSTOM_KEYBINDING_ID_PATTERN,
  type ResolvedCustomKeybinding
} from './custom-keybindings'
import {
  findKeybindingConflicts,
  keybindingChordHasNoNonShiftModifiers,
  keybindingFromInputForCustom,
  normalizeCustomKeybindingChord,
  type KeybindingInput
} from './keybindings'

function sendTextEntry(overrides: Partial<ResolvedCustomKeybinding> = {}): ResolvedCustomKeybinding {
  return {
    id: 'custom.abcd1234efgh',
    title: 'Test entry',
    action: { type: 'sendText', text: '.' },
    bindings: ['Period'],
    decodedText: '.',
    ...overrides
  }
}

function input(partial: Partial<KeybindingInput>): KeybindingInput {
  return { key: '', code: '', alt: false, meta: false, control: false, shift: false, ...partial }
}

describe('decodeCustomSendText', () => {
  it('decodes \\e, \\xNN, \\uNNNN, \\u{...}, \\n, \\r, \\t, \\0, \\\\', () => {
    expect(decodeCustomSendText('\\e[13;2u')).toEqual({ ok: true, text: '\x1b[13;2u' })
    expect(decodeCustomSendText('\\x1b')).toEqual({ ok: true, text: '\x1b' })
    expect(decodeCustomSendText('\\x7f')).toEqual({ ok: true, text: '\x7f' })
    expect(decodeCustomSendText('\\u001b')).toEqual({ ok: true, text: '\x1b' })
    expect(decodeCustomSendText('\\u{1F600}')).toEqual({ ok: true, text: '😀' })
    expect(decodeCustomSendText('\\u{10FFFF}')).toEqual({
      ok: true,
      text: String.fromCodePoint(0x10ffff)
    })
    expect(decodeCustomSendText('a\\nb\\rc\\td\\0e\\\\f')).toEqual({
      ok: true,
      text: 'a\nb\rc\td\0e\\f'
    })
    expect(decodeCustomSendText('plain text')).toEqual({ ok: true, text: 'plain text' })
  })

  it('rejects unknown escapes, bad hex, out-of-range \\u{}, empty result', () => {
    expect(decodeCustomSendText('\\q').ok).toBe(false)
    expect(decodeCustomSendText('\\x1').ok).toBe(false)
    expect(decodeCustomSendText('\\xzz').ok).toBe(false)
    expect(decodeCustomSendText('\\u12').ok).toBe(false)
    expect(decodeCustomSendText('\\u{}').ok).toBe(false)
    expect(decodeCustomSendText('\\u{110000}').ok).toBe(false)
    expect(decodeCustomSendText('\\u{1F600').ok).toBe(false)
    expect(decodeCustomSendText('').ok).toBe(false)
    expect(decodeCustomSendText('\\').ok).toBe(false)
  })
})

describe('normalizeCustomKeybindingChord', () => {
  it('accepts bare Period / Shift+Q / Mod+Alt+K', () => {
    expect(normalizeCustomKeybindingChord('Period')).toEqual({ ok: true, value: 'Period' })
    expect(normalizeCustomKeybindingChord('.')).toEqual({ ok: true, value: 'Period' })
    expect(normalizeCustomKeybindingChord('Shift+Q')).toEqual({ ok: true, value: 'Shift+Q' })
    expect(normalizeCustomKeybindingChord('Mod+Alt+K')).toEqual({ ok: true, value: 'Mod+Alt+K' })
    expect(normalizeCustomKeybindingChord('F5')).toEqual({ ok: true, value: 'F5' })
  })

  it('rejects DoubleTap chords with a custom-specific message', () => {
    const result = normalizeCustomKeybindingChord('DoubleTap+Cmd')
    expect(result).toEqual({
      ok: false,
      error: 'Double-tap shortcuts are not supported for custom shortcuts.'
    })
  })

  it('still rejects unparseable chords', () => {
    expect(normalizeCustomKeybindingChord('NotAKey+Q').ok).toBe(false)
  })
})

describe('keybindingFromInputForCustom', () => {
  it('captures a bare printable key', () => {
    expect(keybindingFromInputForCustom(input({ key: '.', code: 'Period' }), 'darwin')).toEqual({
      ok: true,
      value: 'Period'
    })
  })

  it('rejects a double-tap capture', () => {
    expect(
      keybindingFromInputForCustom(input({ doubleTapModifier: 'Cmd' }), 'darwin').ok
    ).toBe(false)
  })
})

describe('keybindingChordHasNoNonShiftModifiers', () => {
  it('is true for bare and Shift-only chords, false for modified and double-tap chords', () => {
    expect(keybindingChordHasNoNonShiftModifiers('Period')).toBe(true)
    expect(keybindingChordHasNoNonShiftModifiers('Shift+Q')).toBe(true)
    expect(keybindingChordHasNoNonShiftModifiers('Mod+P')).toBe(false)
    expect(keybindingChordHasNoNonShiftModifiers('Ctrl+Alt+K')).toBe(false)
    expect(keybindingChordHasNoNonShiftModifiers('DoubleTap+Cmd')).toBe(false)
  })
})

describe('matchCustomKeybinding', () => {
  it('matches a logical chord and reports the matched binding', () => {
    const entry = sendTextEntry({ bindings: ['Mod+Alt+B', 'Period'] })
    const match = matchCustomKeybinding(
      [entry],
      input({ key: 'b', code: 'KeyB', meta: true, alt: false }),
      'darwin'
    )
    expect(match).toBeNull()
    const altMatch = matchCustomKeybinding(
      [entry],
      input({ key: '', code: 'KeyB', meta: true, alt: true }),
      'darwin'
    )
    expect(altMatch?.entry).toBe(entry)
    expect(altMatch?.binding).toBe('Mod+Alt+B')
    const bare = matchCustomKeybinding([entry], input({ key: '.', code: 'Period' }), 'darwin')
    expect(bare?.binding).toBe('Period')
  })

  it('does not match on modifier mismatch', () => {
    const entry = sendTextEntry({ bindings: ['Shift+Enter'] })
    expect(
      matchCustomKeybinding([entry], input({ key: 'Enter', code: 'Enter' }), 'darwin')
    ).toBeNull()
    expect(
      matchCustomKeybinding(
        [entry],
        input({ key: 'Enter', code: 'Enter', shift: true, control: true }),
        'darwin'
      )
    ).toBeNull()
  })

  it('first entry wins on identical chords', () => {
    const first = sendTextEntry({ id: 'custom.first0000001', bindings: ['Shift+Enter'] })
    const second = sendTextEntry({ id: 'custom.second000001', bindings: ['Shift+Enter'] })
    const match = matchCustomKeybinding(
      [first, second],
      input({ key: 'Enter', code: 'Enter', shift: true }),
      'darwin'
    )
    expect(match?.entry.id).toBe('custom.first0000001')
  })

  it('matchPhysicalKey matches key 。 code Period and key ， code Comma (repro #9338)', () => {
    const period = sendTextEntry({ bindings: ['Period'], matchPhysicalKey: true })
    const comma = sendTextEntry({
      id: 'custom.comma0000001',
      action: { type: 'sendText', text: ',' },
      decodedText: ',',
      bindings: ['Comma'],
      matchPhysicalKey: true
    })
    expect(
      matchCustomKeybinding([period, comma], input({ key: '。', code: 'Period' }), 'darwin')?.entry
    ).toBe(period)
    expect(
      matchCustomKeybinding([period, comma], input({ key: '，', code: 'Comma' }), 'darwin')?.entry
    ).toBe(comma)
  })

  it('without matchPhysicalKey the composed char does not match', () => {
    const period = sendTextEntry({ bindings: ['Period'] })
    expect(
      matchCustomKeybinding([period], input({ key: '。', code: 'Period' }), 'darwin')
    ).toBeNull()
  })

  it('matchPhysicalKey does not fire on Ctrl+Alt (AltGr guard preserved)', () => {
    const period = sendTextEntry({ bindings: ['Period'], matchPhysicalKey: true })
    expect(
      matchCustomKeybinding(
        [period],
        input({ key: '。', code: 'Period', control: true, alt: true }),
        'win32'
      )
    ).toBeNull()
  })
})

describe('findKeybindingConflicts with custom entries', () => {
  it('reports a custom entry chord colliding with terminal.copySelection with both ids', () => {
    const custom = sendTextEntry({ bindings: ['Mod+Shift+C'], title: 'My macro' })
    const conflicts = findKeybindingConflicts('darwin', {}, {}, [custom])
    const conflict = conflicts.find((candidate) => candidate.actionIds.includes(custom.id))
    expect(conflict).toBeDefined()
    expect(conflict?.actionIds).toContain('terminal.copySelection')
    expect(resolveKeybindingTitle('terminal.copySelection', [custom])).toBe(
      'Copy terminal selection'
    )
    expect(resolveKeybindingTitle(custom.id, [custom])).toBe('My macro')
    expect(resolveKeybindingTitle('custom.unknown00001', [custom])).toBe('custom.unknown00001')
  })

  it('reports custom↔custom collisions', () => {
    const first = sendTextEntry({ id: 'custom.first0000001', bindings: ['Mod+Alt+M'] })
    const second = sendTextEntry({ id: 'custom.second000001', bindings: ['Mod+Alt+M'] })
    const conflicts = findKeybindingConflicts('darwin', {}, {}, [first, second])
    expect(
      conflicts.some(
        (conflict) =>
          conflict.actionIds.includes(first.id) && conflict.actionIds.includes(second.id)
      )
    ).toBe(true)
  })

  it('does not report a bare printable custom chord as a conflict (shadowing is a warning, not a conflict)', () => {
    const custom = sendTextEntry({ bindings: ['Period'] })
    expect(findKeybindingConflicts('darwin', {}, {}, [custom])).toEqual([])
  })

  it('does not conflict with non-terminal scopes', () => {
    // Mod+P belongs to worktree.quickOpen (global scope); custom entries are terminal-scope.
    const custom = sendTextEntry({ bindings: ['Mod+P'] })
    expect(findKeybindingConflicts('darwin', {}, {}, [custom])).toEqual([])
  })
})

describe('custom keybinding ids', () => {
  it('generates ids matching the reserved pattern', () => {
    const id = generateCustomKeybindingId()
    expect(CUSTOM_KEYBINDING_ID_PATTERN.test(id)).toBe(true)
    expect(isCustomKeybindingId(id)).toBe(true)
    expect(isCustomKeybindingId('terminal.copySelection')).toBe(false)
  })
})
