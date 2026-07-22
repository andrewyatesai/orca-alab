import React, { useState } from 'react'
import { formatKeybinding } from '../../../../shared/keybindings'
import type { ResolvedCustomKeybinding } from '../../../../shared/custom-keybindings'
import { useAppStore } from '../../store'
import { Button } from '../ui/button'
import { ShortcutKeyCombo } from '../ShortcutKeyCombo'
import { SettingsSubsectionHeader } from './SettingsFormControls'
import { bareChordShadowWarning, CustomShortcutEditor } from './CustomShortcutEditor'
import { translate } from '@/i18n/i18n'

type CustomShortcutsSectionProps = {
  platform: NodeJS.Platform
  /** Conflict messages keyed by action id (built-in and custom), from ShortcutsPane's shared map. */
  conflictByAction: ReadonlyMap<string, string[]>
  /** Local shortcut search query; rows filter with the same text the grid uses. */
  query: string
}

function actionSummary(entry: ResolvedCustomKeybinding, quickCommandLabel: string | null): string {
  if (entry.action.type === 'sendText') {
    return translate(
      'auto.components.settings.CustomShortcutsSection.sends',
      'Sends "{{value0}}"',
      { value0: entry.action.text }
    )
  }
  return translate('auto.components.settings.CustomShortcutsSection.runs', 'Runs "{{value0}}"', {
    value0: quickCommandLabel ?? entry.action.quickCommandId
  })
}

function matchesQuery(entry: ResolvedCustomKeybinding, query: string): boolean {
  if (!query) {
    return true
  }
  const payload = entry.action.type === 'sendText' ? entry.action.text : entry.action.quickCommandId
  const haystack = [entry.title, 'custom', 'macro', 'send text', payload].join(' ').toLowerCase()
  return query
    .toLowerCase()
    .split(/\s+/)
    .every((term) => haystack.includes(term))
}

export function CustomShortcutsSection({
  platform,
  conflictByAction,
  query
}: CustomShortcutsSectionProps): React.JSX.Element {
  const customKeybindings = useAppStore((state) => state.customKeybindings)
  const removeCustomKeybinding = useAppStore((state) => state.removeCustomKeybinding)
  const quickCommands = useAppStore((state) => state.settings?.terminalQuickCommands ?? [])
  const [editorOpen, setEditorOpen] = useState(false)
  const [editingEntry, setEditingEntry] = useState<ResolvedCustomKeybinding | null>(null)

  const visibleEntries = customKeybindings.filter((entry) => matchesQuery(entry, query.trim()))

  const warningsFor = (entry: ResolvedCustomKeybinding): string[] => {
    const warnings = entry.bindings
      .map((binding) => bareChordShadowWarning(binding, platform))
      .filter((warning): warning is string => warning !== null)
    const action = entry.action
    if (
      action.type === 'runQuickCommand' &&
      !quickCommands.some((command) => command.id === action.quickCommandId)
    ) {
      warnings.push(
        translate(
          'auto.components.settings.CustomShortcutsSection.danglingQuickCommand',
          'quick command no longer exists.'
        )
      )
    }
    return warnings
  }

  return (
    <section className="space-y-3" data-testid="custom-shortcuts-section">
      <SettingsSubsectionHeader
        title={translate(
          'auto.components.settings.CustomShortcutsSection.title',
          'Custom Shortcuts'
        )}
        description={translate(
          'auto.components.settings.CustomShortcutsSection.description',
          'Send text or run a quick command with a shortcut of your own.'
        )}
        action={
          <Button
            size="sm"
            variant="outline"
            onClick={() => {
              setEditingEntry(null)
              setEditorOpen(true)
            }}
          >
            {translate(
              'auto.components.settings.CustomShortcutsSection.add',
              'Add Custom Shortcut'
            )}
          </Button>
        }
      />

      {visibleEntries.length > 0 ? (
        <ul className="max-h-56 space-y-1 overflow-y-auto pr-1 scrollbar-sleek">
          {visibleEntries.map((entry) => {
            const warnings = warningsFor(entry)
            const conflicts = conflictByAction.get(entry.id) ?? []
            const action = entry.action
            const quickCommandLabel =
              action.type === 'runQuickCommand'
                ? (quickCommands.find((command) => command.id === action.quickCommandId)?.label ??
                  null)
                : null
            return (
              <li
                key={entry.id}
                className="flex items-start justify-between gap-3 rounded-md border border-transparent px-2 py-1.5 hover:border-border/70 hover:bg-background"
              >
                <div className="min-w-0 space-y-0.5">
                  <p className="truncate text-xs font-medium">{entry.title}</p>
                  <p className="truncate font-mono text-[11px] text-muted-foreground">
                    {actionSummary(entry, quickCommandLabel)}
                  </p>
                  {warnings.map((warning) => (
                    <p key={warning} className="text-[11px] text-muted-foreground" role="alert">
                      ⚠ {warning}
                    </p>
                  ))}
                  {conflicts.map((message) => (
                    <p key={message} className="text-[11px] text-destructive" role="alert">
                      {message}
                    </p>
                  ))}
                </div>
                <div className="flex shrink-0 items-center gap-1.5">
                  <span className="flex flex-wrap items-center justify-end gap-1.5">
                    {entry.bindings.map((binding) => (
                      <ShortcutKeyCombo key={binding} keys={formatKeybinding(binding, platform)} />
                    ))}
                  </span>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => {
                      setEditingEntry(entry)
                      setEditorOpen(true)
                    }}
                  >
                    {translate('auto.components.settings.CustomShortcutsSection.edit', 'Edit')}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    aria-label={translate(
                      'auto.components.settings.CustomShortcutsSection.deleteFor',
                      'Delete {{value0}}',
                      { value0: entry.title }
                    )}
                    onClick={() => void removeCustomKeybinding(entry.id)}
                  >
                    {translate('auto.components.settings.CustomShortcutsSection.delete', 'Delete')}
                  </Button>
                </div>
              </li>
            )
          })}
        </ul>
      ) : (
        <p className="text-xs text-muted-foreground">
          {customKeybindings.length === 0
            ? translate(
                'auto.components.settings.CustomShortcutsSection.empty',
                'No custom shortcuts yet.'
              )
            : translate(
                'auto.components.settings.CustomShortcutsSection.noMatches',
                'No custom shortcuts match the search.'
              )}
        </p>
      )}

      <CustomShortcutEditor
        open={editorOpen}
        platform={platform}
        entry={editingEntry}
        onClose={() => setEditorOpen(false)}
      />
    </section>
  )
}
