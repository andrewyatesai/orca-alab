import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { LEGACY_TAB_SWITCH_BINDINGS } from '../../shared/keybindings'
import {
  getUserKeybindingsPath,
  migrateLegacyKeybindings,
  readKeybindingFile,
  removeCustomKeybinding,
  seedLegacyTabSwitchBindings,
  upsertCustomKeybinding,
  writeKeybindingOverride
} from './keybinding-file'

describe('keybinding-file', () => {
  let dir: string
  let filePath: string

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), 'orca-keybindings-'))
    filePath = join(dir, 'keybindings.json')
  })

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true })
  })

  it('resolves the user-facing keybindings path under ~/.orca', () => {
    expect(getUserKeybindingsPath('/home/test')).toBe(
      join('/home/test', '.orca', 'keybindings.json')
    )
  })

  it('returns an empty snapshot when the file does not exist', () => {
    expect(readKeybindingFile(filePath, 'linux')).toMatchObject({
      exists: false,
      platform: 'linux',
      overrides: {},
      diagnostics: []
    })
  })

  it('parses common and platform-specific overrides', () => {
    writeFileSync(
      filePath,
      JSON.stringify({
        version: 1,
        keybindings: {
          'worktree.quickOpen': 'Mod+Shift+P',
          'view.tasks': null
        },
        platforms: {
          linux: {
            'terminal.paste': ['Ctrl+Shift+V', 'Shift+Insert'],
            'terminal.search': 'Ctrl+Shift+F'
          },
          darwin: {
            'terminal.search': 'Mod+F'
          }
        }
      }),
      'utf8'
    )

    expect(readKeybindingFile(filePath, 'linux')).toMatchObject({
      exists: true,
      overrides: {
        'worktree.quickOpen': ['Mod+Shift+P'],
        'view.tasks': [],
        'terminal.paste': ['Ctrl+Shift+V', 'Shift+Insert'],
        'terminal.search': ['Ctrl+Shift+F']
      },
      diagnostics: []
    })
  })

  it('accepts bare keys for actions that explicitly opt in', () => {
    writeFileSync(
      filePath,
      JSON.stringify({
        keybindings: {
          'fileExplorer.delete': 'Delete'
        }
      }),
      'utf8'
    )

    expect(readKeybindingFile(filePath, 'linux')).toMatchObject({
      overrides: {
        'fileExplorer.delete': ['Delete']
      },
      diagnostics: []
    })
  })

  it('ignores invalid, unknown, and conflicting manual edits', () => {
    writeFileSync(
      filePath,
      JSON.stringify({
        keybindings: {
          unknownAction: 'Ctrl+Alt+U',
          'terminal.search': 'not-a-keybinding',
          'view.tasks': 'Mod+P'
        }
      }),
      'utf8'
    )

    const snapshot = readKeybindingFile(filePath, 'linux')

    expect(snapshot.overrides).toEqual({})
    expect(snapshot.diagnostics.map((diagnostic) => diagnostic.severity)).toEqual([
      'warning',
      'error',
      'error'
    ])
  })

  it('writes active-platform overrides while preserving other platforms', () => {
    writeFileSync(
      filePath,
      JSON.stringify({
        version: 1,
        keybindings: {
          'worktree.quickOpen': 'Mod+Shift+P'
        },
        platforms: {
          darwin: {
            'terminal.search': 'Mod+F'
          }
        }
      }),
      'utf8'
    )

    writeKeybindingOverride(filePath, 'linux', 'terminal.search', ['Ctrl+Shift+F'])

    const written = JSON.parse(readFileSync(filePath, 'utf8')) as {
      keybindings: Record<string, unknown>
      platforms: Record<string, Record<string, unknown>>
    }
    expect(written.keybindings['worktree.quickOpen']).toBe('Mod+Shift+P')
    expect(written.platforms.darwin['terminal.search']).toBe('Mod+F')
    expect(written.platforms.linux['terminal.search']).toEqual(['Ctrl+Shift+F'])
  })

  it('migrates root-level legacy overrides before writing settings edits', () => {
    writeFileSync(
      filePath,
      JSON.stringify({
        version: 1,
        'worktree.quickOpen': 'Mod+Shift+P',
        platforms: {
          darwin: {
            'terminal.search': 'Mod+F'
          }
        }
      }),
      'utf8'
    )

    writeKeybindingOverride(filePath, 'linux', 'terminal.search', ['Ctrl+Shift+F'])

    const written = JSON.parse(readFileSync(filePath, 'utf8')) as {
      keybindings: Record<string, unknown>
      platforms: Record<string, Record<string, unknown>>
      'worktree.quickOpen'?: unknown
    }
    expect(written['worktree.quickOpen']).toBeUndefined()
    expect(written.keybindings['worktree.quickOpen']).toEqual(['Mod+Shift+P'])
    expect(written.platforms.darwin['terminal.search']).toBe('Mod+F')
    expect(readKeybindingFile(filePath, 'linux').overrides).toEqual({
      'worktree.quickOpen': ['Mod+Shift+P'],
      'terminal.search': ['Ctrl+Shift+F']
    })
  })

  it('rejects writes that would conflict with another effective shortcut', () => {
    expect(() => writeKeybindingOverride(filePath, 'linux', 'view.tasks', ['Mod+P'])).toThrow(
      'conflicts with another shortcut'
    )
    expect(readKeybindingFile(filePath, 'linux').overrides).toEqual({})
  })

  it('validates write inputs at the file boundary', () => {
    expect(() => writeKeybindingOverride(filePath, 'linux', 'unknown.action', [])).toThrow(
      'Unknown keybinding action'
    )
    expect(() => writeKeybindingOverride(filePath, 'linux', 'view.tasks', 'Ctrl+Alt+T')).toThrow(
      'Use a string array or null.'
    )
  })

  it('resets only the active platform override', () => {
    writeFileSync(
      filePath,
      JSON.stringify({
        keybindings: {
          'terminal.search': 'Ctrl+Alt+F'
        },
        platforms: {
          linux: {
            'terminal.search': 'Ctrl+Shift+F'
          }
        }
      }),
      'utf8'
    )

    writeKeybindingOverride(filePath, 'linux', 'terminal.search', null)

    const snapshot = readKeybindingFile(filePath, 'linux')
    expect(snapshot.commonOverrides).toEqual({
      'terminal.search': ['Ctrl+Alt+F']
    })
    expect(snapshot.platformOverrides.linux).toEqual({})
    expect(snapshot.overrides).toEqual({
      'terminal.search': ['Ctrl+Alt+F']
    })
  })

  it('migrates legacy settings once when no file exists', () => {
    migrateLegacyKeybindings(filePath, 'linux', { 'view.tasks': ['Ctrl+Alt+T'] })
    migrateLegacyKeybindings(filePath, 'linux', { 'view.tasks': ['Ctrl+Alt+X'] })

    expect(readKeybindingFile(filePath, 'linux').overrides).toEqual({
      'view.tasks': ['Ctrl+Alt+T']
    })
  })

  it('seeds the legacy tab-switch chords into the active platform section', () => {
    const result = seedLegacyTabSwitchBindings(filePath, 'darwin', LEGACY_TAB_SWITCH_BINDINGS)

    expect(result.seeded).toBe(true)
    const snapshot = readKeybindingFile(filePath, 'darwin')
    // Written to the platform section so Settings reset still works normally.
    expect(snapshot.platformOverrides.darwin).toEqual({
      'tab.nextSameType': ['Mod+Shift+BracketRight'],
      'tab.previousSameType': ['Mod+Shift+BracketLeft'],
      'tab.nextAllTypes': ['Mod+Alt+BracketRight'],
      'tab.previousAllTypes': ['Mod+Alt+BracketLeft']
    })
    // Effective bindings reproduce the pre-swap behavior with no conflicts dropped.
    expect(snapshot.overrides).toMatchObject({
      'tab.nextSameType': ['Mod+Shift+BracketRight'],
      'tab.nextAllTypes': ['Mod+Alt+BracketRight']
    })
    expect(snapshot.diagnostics).toEqual([])
  })

  it('preserves a customized action while still pinning the un-customized ones', () => {
    // A partially-customized existing user: they rebound one action. The other
    // three must still land on their pre-swap defaults, not the new ones.
    writeKeybindingOverride(filePath, 'darwin', 'tab.nextSameType', ['Mod+Alt+K'])

    const result = seedLegacyTabSwitchBindings(filePath, 'darwin', LEGACY_TAB_SWITCH_BINDINGS)

    expect(result.seeded).toBe(true)
    expect(readKeybindingFile(filePath, 'darwin').overrides).toEqual({
      'tab.nextSameType': ['Mod+Alt+K'],
      'tab.previousSameType': ['Mod+Shift+BracketLeft'],
      'tab.nextAllTypes': ['Mod+Alt+BracketRight'],
      'tab.previousAllTypes': ['Mod+Alt+BracketLeft']
    })
  })

  it('preserves pre-swap customizations that conflict only with the new defaults', () => {
    // This was valid before the swap: moving nextSameType to Mod+K freed its
    // old Shift+] chord for previousSameType. The new registry temporarily
    // assigns Shift+] to nextAllTypes, but the seed must not mistake the
    // resulting conflict for an absent user override and replace it.
    writeFileSync(
      filePath,
      JSON.stringify({
        version: 1,
        platforms: {
          darwin: {
            'tab.nextSameType': ['Mod+K'],
            'tab.previousSameType': ['Mod+Shift+BracketRight']
          }
        }
      }),
      'utf8'
    )

    seedLegacyTabSwitchBindings(filePath, 'darwin', LEGACY_TAB_SWITCH_BINDINGS)

    expect(readKeybindingFile(filePath, 'darwin')).toMatchObject({
      overrides: {
        'tab.nextSameType': ['Mod+K'],
        'tab.previousSameType': ['Mod+Shift+BracketRight'],
        'tab.nextAllTypes': ['Mod+Alt+BracketRight'],
        'tab.previousAllTypes': ['Mod+Alt+BracketLeft']
      },
      diagnostics: []
    })
  })

  it('is a no-op once every swapped action already resolves on this platform', () => {
    // Seeding writes the whole document at once, so it clears the per-write
    // conflict guard that a naive one-action-at-a-time write would trip.
    const first = seedLegacyTabSwitchBindings(filePath, 'darwin', LEGACY_TAB_SWITCH_BINDINGS)
    expect(first.seeded).toBe(true)
    expect(first.snapshot.diagnostics).toEqual([])

    const second = seedLegacyTabSwitchBindings(filePath, 'darwin', LEGACY_TAB_SWITCH_BINDINGS)
    expect(second.seeded).toBe(false)
  })

  it('does not let a foreign-platform override block the active-platform pin', () => {
    // Only linux is customized; a darwin launch must still pin darwin so the
    // active platform keeps the pre-swap behavior.
    writeKeybindingOverride(filePath, 'linux', 'tab.nextAllTypes', ['Mod+Alt+K'])

    const result = seedLegacyTabSwitchBindings(filePath, 'darwin', LEGACY_TAB_SWITCH_BINDINGS)

    expect(result.seeded).toBe(true)
    expect(readKeybindingFile(filePath, 'darwin').platformOverrides.darwin).toMatchObject({
      'tab.nextAllTypes': ['Mod+Alt+BracketRight']
    })
    expect(readKeybindingFile(filePath, 'linux').platformOverrides.linux).toEqual({
      'tab.nextAllTypes': ['Mod+Alt+K']
    })
  })

  it('leaves unrelated overrides intact and stays idempotent when seeding', () => {
    writeKeybindingOverride(filePath, 'darwin', 'terminal.search', ['Mod+Shift+F'])

    const first = seedLegacyTabSwitchBindings(filePath, 'darwin', LEGACY_TAB_SWITCH_BINDINGS)
    const second = seedLegacyTabSwitchBindings(filePath, 'darwin', LEGACY_TAB_SWITCH_BINDINGS)

    expect(first.seeded).toBe(true)
    // A second pass is a no-op: the pins now read as existing customization.
    expect(second.seeded).toBe(false)
    const snapshot = readKeybindingFile(filePath, 'darwin')
    expect(snapshot.overrides['terminal.search']).toEqual(['Mod+Shift+F'])
    expect(snapshot.overrides['tab.nextAllTypes']).toEqual(['Mod+Alt+BracketRight'])
  })

  it('migrates root-level legacy overrides without losing custom shortcuts', () => {
    writeFileSync(
      filePath,
      JSON.stringify({
        version: 1,
        'worktree.quickOpen': 'Mod+Shift+P',
        'tab.nextSameType': 'Mod+K'
      }),
      'utf8'
    )

    seedLegacyTabSwitchBindings(filePath, 'darwin', LEGACY_TAB_SWITCH_BINDINGS)

    const written = JSON.parse(readFileSync(filePath, 'utf8')) as {
      keybindings: Record<string, unknown>
      platforms: Record<string, Record<string, unknown>>
      'worktree.quickOpen'?: unknown
      'tab.nextSameType'?: unknown
    }
    expect(written['worktree.quickOpen']).toBeUndefined()
    expect(written['tab.nextSameType']).toBeUndefined()
    expect(written.keybindings).toMatchObject({
      'worktree.quickOpen': ['Mod+Shift+P'],
      'tab.nextSameType': ['Mod+K']
    })
    expect(readKeybindingFile(filePath, 'darwin').overrides).toMatchObject({
      'worktree.quickOpen': ['Mod+Shift+P'],
      'tab.nextSameType': ['Mod+K'],
      'tab.nextAllTypes': ['Mod+Alt+BracketRight']
    })
  })

  it('throws instead of freezing the seed when a legacy binding fails normalization', () => {
    // A dropped pin must not be silent: the service catches the throw and keeps
    // the cohort pending so a corrected build can retry the seed. Valid pins
    // from the same batch are still written so users keep the good shortcuts.
    expect(() =>
      seedLegacyTabSwitchBindings(filePath, 'darwin', {
        'tab.nextSameType': ['J'],
        'tab.previousSameType': ['Mod+Shift+BracketLeft']
      })
    ).toThrow(/normalize/i)
    expect(readKeybindingFile(filePath, 'darwin').platformOverrides.darwin).toEqual({
      'tab.previousSameType': ['Mod+Shift+BracketLeft']
    })
  })

  it('does not replace an unreadable keybindings file while seeding', () => {
    const unreadableContents = '{{{not json'
    writeFileSync(filePath, unreadableContents, 'utf8')

    expect(() =>
      seedLegacyTabSwitchBindings(filePath, 'darwin', LEGACY_TAB_SWITCH_BINDINGS)
    ).toThrow()
    expect(readFileSync(filePath, 'utf8')).toBe(unreadableContents)
  })

  describe('custom section', () => {
    const validEntry = {
      id: 'custom.k3v9x2m1q8za',
      title: 'ASCII period (CJK remap)',
      action: { type: 'sendText', text: '.' },
      bindings: ['Period'],
      matchPhysicalKey: true
    }

    it('parses the custom section; a missing section yields []', () => {
      expect(readKeybindingFile(filePath, 'darwin').custom).toEqual([])

      writeFileSync(
        filePath,
        JSON.stringify({
          version: 1,
          keybindings: {},
          custom: [
            validEntry,
            {
              id: 'custom.p0f4h7n2w6yb',
              title: 'Kitty Shift+Enter',
              action: { type: 'sendText', text: '\\x1b[13;2u' },
              bindings: ['Shift+Enter']
            },
            {
              id: 'custom.d8s1r5c3j9te',
              title: 'Run: rebuild',
              action: { type: 'runQuickCommand', quickCommandId: 'qc-rebuild' },
              bindings: ['Mod+Alt+B']
            }
          ]
        }),
        'utf8'
      )

      const snapshot = readKeybindingFile(filePath, 'darwin')
      expect(snapshot.custom).toHaveLength(3)
      expect(snapshot.custom[0]).toMatchObject({
        id: 'custom.k3v9x2m1q8za',
        matchPhysicalKey: true,
        decodedText: '.'
      })
      expect(snapshot.custom[1].decodedText).toBe('\x1b[13;2u')
      expect(snapshot.custom[2].decodedText).toBeUndefined()
      expect(snapshot.diagnostics.filter((d) => d.severity === 'error')).toEqual([])
    })

    it('drops entries with bad id, duplicate id, bad action, or oversized payload with custom diagnostics', () => {
      writeFileSync(
        filePath,
        JSON.stringify({
          version: 1,
          keybindings: {},
          custom: [
            { ...validEntry, id: 'notcustom.zzzz' },
            validEntry,
            { ...validEntry, id: 'custom.k3v9x2m1q8za' },
            { ...validEntry, id: 'custom.badaction001', action: { type: 'sendText' } },
            { ...validEntry, id: 'custom.badescape001', action: { type: 'sendText', text: '\\q' } },
            {
              ...validEntry,
              id: 'custom.oversized001',
              action: { type: 'sendText', text: 'x'.repeat(5000) }
            },
            'not-an-object'
          ]
        }),
        'utf8'
      )

      const snapshot = readKeybindingFile(filePath, 'darwin')
      expect(snapshot.custom.map((entry) => entry.id)).toEqual(['custom.k3v9x2m1q8za'])
      const errors = snapshot.diagnostics.filter(
        (d) => d.severity === 'error' && d.section === 'custom'
      )
      expect(errors).toHaveLength(6)
      expect(errors.map((d) => d.message).join('\n')).toMatch(/duplicate id/)
      expect(errors.map((d) => d.message).join('\n')).toMatch(/bytes/)
    })

    it('keeps an entry with a bare-printable chord and emits a warning diagnostic', () => {
      writeFileSync(
        filePath,
        JSON.stringify({ version: 1, keybindings: {}, custom: [validEntry] }),
        'utf8'
      )
      const snapshot = readKeybindingFile(filePath, 'darwin')
      expect(snapshot.custom).toHaveLength(1)
      expect(
        snapshot.diagnostics.some(
          (d) =>
            d.severity === 'warning' &&
            d.section === 'custom' &&
            /no longer type its character/.test(d.message)
        )
      ).toBe(true)
    })

    it('load-time conflict removes only the custom entry offending binding, override untouched', () => {
      writeFileSync(
        filePath,
        JSON.stringify({
          version: 1,
          keybindings: { 'terminal.clear': ['Mod+Shift+X'] },
          custom: [
            {
              id: 'custom.conflicted01',
              title: 'Clashes with clear',
              action: { type: 'sendText', text: 'ls\\n' },
              bindings: ['Mod+Shift+X', 'Mod+Alt+Y']
            }
          ]
        }),
        'utf8'
      )

      const snapshot = readKeybindingFile(filePath, 'darwin')
      expect(snapshot.overrides['terminal.clear']).toEqual(['Mod+Shift+X'])
      expect(snapshot.custom).toHaveLength(1)
      expect(snapshot.custom[0].bindings).toEqual(['Mod+Alt+Y'])
      expect(
        snapshot.diagnostics.some(
          (d) =>
            d.severity === 'error' &&
            d.section === 'custom' &&
            d.message.includes('Clashes with clear') &&
            d.message.includes('Clear active pane')
        )
      ).toBe(true)
    })

    it('load-time custom↔custom conflict keeps the first entry chord', () => {
      const clashing = (id: string, title: string) => ({
        id,
        title,
        action: { type: 'sendText', text: 'x' },
        bindings: ['Mod+Alt+Z']
      })
      writeFileSync(
        filePath,
        JSON.stringify({
          version: 1,
          keybindings: {},
          custom: [clashing('custom.first0000001', 'First'), clashing('custom.second000001', 'Second')]
        }),
        'utf8'
      )
      const snapshot = readKeybindingFile(filePath, 'darwin')
      expect(snapshot.custom[0].bindings).toEqual(['Mod+Alt+Z'])
      expect(snapshot.custom[1].bindings).toEqual([])
    })

    it('upsertCustomKeybinding round-trips, preserving platforms, unknown root keys, and when clause', () => {
      writeFileSync(
        filePath,
        JSON.stringify({
          version: 1,
          keybindings: {},
          platforms: { darwin: { 'terminal.search': ['Mod+F'] }, linux: {}, win32: {} },
          futureRootKey: { keep: true },
          custom: [
            {
              ...validEntry,
              when: { connection: 'ssh' },
              futureEntryKey: 'keep-me'
            }
          ]
        }),
        'utf8'
      )

      const snapshot = upsertCustomKeybinding(filePath, 'darwin', {
        id: 'custom.k3v9x2m1q8za',
        title: 'Renamed remap',
        action: { type: 'sendText', text: ',' },
        bindings: ['comma'],
        matchPhysicalKey: true
      })
      expect(snapshot.custom).toHaveLength(1)
      expect(snapshot.custom[0]).toMatchObject({
        title: 'Renamed remap',
        bindings: ['Comma'],
        decodedText: ',',
        when: { connection: 'ssh' }
      })

      const written = JSON.parse(readFileSync(filePath, 'utf8')) as Record<string, unknown>
      expect(written.futureRootKey).toEqual({ keep: true })
      expect(written.platforms).toMatchObject({ darwin: { 'terminal.search': ['Mod+F'] } })
      const writtenCustom = written.custom as Record<string, unknown>[]
      expect(writtenCustom[0].futureEntryKey).toBe('keep-me')
      expect(writtenCustom[0].when).toEqual({ connection: 'ssh' })
      // Stored bindings are canonical; stored text is the raw escaped form, never decoded bytes.
      expect(writtenCustom[0].bindings).toEqual(['Comma'])
      expect(writtenCustom[0].action).toEqual({ type: 'sendText', text: ',' })
      expect(writtenCustom[0].decodedText).toBeUndefined()

      const appended = upsertCustomKeybinding(filePath, 'darwin', {
        id: 'custom.appended0001',
        title: 'Second entry',
        action: { type: 'runQuickCommand', quickCommandId: 'qc-1' },
        bindings: ['Mod+Alt+9']
      })
      expect(appended.custom.map((entry) => entry.id)).toEqual([
        'custom.k3v9x2m1q8za',
        'custom.appended0001'
      ])
    })

    it('upsertCustomKeybinding throws on a blocking conflict with a built-in override', () => {
      writeFileSync(
        filePath,
        JSON.stringify({ version: 1, keybindings: { 'terminal.clear': ['Mod+Shift+X'] } }),
        'utf8'
      )
      expect(() =>
        upsertCustomKeybinding(filePath, 'darwin', {
          id: 'custom.conflicted01',
          title: 'Clash',
          action: { type: 'sendText', text: 'x' },
          bindings: ['Mod+Shift+X']
        })
      ).toThrow(/conflicts with/)
      expect(readKeybindingFile(filePath, 'darwin').custom).toEqual([])
    })

    it('upsertCustomKeybinding throws on invalid entries', () => {
      expect(() =>
        upsertCustomKeybinding(filePath, 'darwin', {
          id: 'custom.badentry0001',
          title: 'Bad',
          action: { type: 'sendText', text: '\\q' },
          bindings: ['Mod+Alt+Q']
        })
      ).toThrow(/Unknown escape/)
      expect(() =>
        upsertCustomKeybinding(filePath, 'darwin', {
          id: 'custom.badentry0002',
          title: 'Bad chord',
          action: { type: 'sendText', text: 'x' },
          bindings: ['DoubleTap+Cmd']
        })
      ).toThrow(/Double-tap/)
    })

    it('removeCustomKeybinding removes only the target id', () => {
      writeFileSync(
        filePath,
        JSON.stringify({
          version: 1,
          keybindings: {},
          custom: [
            validEntry,
            {
              id: 'custom.keepme000001',
              title: 'Keep me',
              action: { type: 'sendText', text: 'x' },
              bindings: ['Mod+Alt+K']
            }
          ]
        }),
        'utf8'
      )
      const snapshot = removeCustomKeybinding(filePath, 'darwin', 'custom.k3v9x2m1q8za')
      expect(snapshot.custom.map((entry) => entry.id)).toEqual(['custom.keepme000001'])
    })

    it('writeKeybindingOverride preserves an existing custom section (downgrade symmetry)', () => {
      writeFileSync(
        filePath,
        JSON.stringify({ version: 1, keybindings: {}, custom: [validEntry] }),
        'utf8'
      )
      writeKeybindingOverride(filePath, 'darwin', 'terminal.search', ['Mod+Shift+F'])
      const written = JSON.parse(readFileSync(filePath, 'utf8')) as Record<string, unknown>
      expect(written.custom).toEqual([validEntry])
    })

    it('writeKeybindingOverride blocks a chord already taken by a custom entry', () => {
      writeFileSync(
        filePath,
        JSON.stringify({
          version: 1,
          keybindings: {},
          custom: [
            {
              id: 'custom.holder000001',
              title: 'Holder',
              action: { type: 'sendText', text: 'x' },
              bindings: ['Mod+Alt+H']
            }
          ]
        }),
        'utf8'
      )
      expect(() =>
        writeKeybindingOverride(filePath, 'darwin', 'terminal.clear', ['Mod+Alt+H'])
      ).toThrow(/conflicts with/)
    })
  })
})
