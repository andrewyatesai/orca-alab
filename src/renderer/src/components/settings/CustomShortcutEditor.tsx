import React, { useEffect, useMemo, useState } from 'react'
import {
  findKeybindingConflicts,
  formatKeybinding,
  keybindingChordHasNoNonShiftModifiers,
  keybindingFromInputForCustom,
  type KeybindingInput
} from '../../../../shared/keybindings'
import {
  decodeCustomSendText,
  generateCustomKeybindingId,
  resolveKeybindingTitle,
  type CustomKeybinding,
  type ResolvedCustomKeybinding
} from '../../../../shared/custom-keybindings'
import { modifierFromKeyEvent } from '../../../../shared/modifier-double-tap-detector'
import { isTerminalAgentQuickCommand } from '@/lib/git-wasm/terminal-quick-commands'
import { useAppStore } from '../../store'
import { Button } from '../ui/button'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle
} from '../ui/dialog'
import { Input } from '../ui/input'
import { Checkbox } from '../ui/checkbox'
import { Label } from '../ui/label'
import { ShortcutKeyCombo } from '../ShortcutKeyCombo'
import {
  CustomShortcutActionFields,
  type CustomShortcutActionType
} from './CustomShortcutActionFields'
import { translate } from '@/i18n/i18n'

const BARE_PUNCTUATION_CHORD_PATTERN =
  /^(BracketLeft|BracketRight|Minus|Underscore|Equal|Plus|Comma|Period|Slash|Backslash|Semicolon|Quote|Backquote)$/

export function bareChordShadowWarning(binding: string, platform: NodeJS.Platform): string | null {
  if (!keybindingChordHasNoNonShiftModifiers(binding)) {
    return null
  }
  return translate(
    'auto.components.settings.CustomShortcutEditor.bareShadow',
    '{{value0}} will no longer type its character in terminals.',
    { value0: formatKeybinding(binding, platform).join('') }
  )
}

type CustomShortcutEditorProps = {
  open: boolean
  platform: NodeJS.Platform
  /** null = create a new entry. */
  entry: ResolvedCustomKeybinding | null
  onClose: () => void
}

export function CustomShortcutEditor({
  open,
  platform,
  entry,
  onClose
}: CustomShortcutEditorProps): React.JSX.Element {
  const keybindings = useAppStore((state) => state.keybindings)
  const customKeybindings = useAppStore((state) => state.customKeybindings)
  const upsertCustomKeybinding = useAppStore((state) => state.upsertCustomKeybinding)
  const quickCommands = useAppStore((state) => state.settings?.terminalQuickCommands ?? [])
  const terminalQuickCommands = useMemo(
    () => quickCommands.filter((command) => !isTerminalAgentQuickCommand(command)),
    [quickCommands]
  )

  const [title, setTitle] = useState('')
  const [actionType, setActionType] = useState<CustomShortcutActionType>('sendText')
  const [sendText, setSendText] = useState('')
  const [quickCommandId, setQuickCommandId] = useState('')
  const [bindings, setBindings] = useState<string[]>([])
  const [matchPhysicalKey, setMatchPhysicalKey] = useState(false)
  const [recording, setRecording] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!open) {
      return
    }
    setTitle(entry?.title ?? '')
    setActionType(entry?.action.type ?? 'sendText')
    setSendText(entry?.action.type === 'sendText' ? entry.action.text : '')
    setQuickCommandId(entry?.action.type === 'runQuickCommand' ? entry.action.quickCommandId : '')
    setBindings(entry?.bindings ?? [])
    setMatchPhysicalKey(entry?.matchPhysicalKey === true)
    setRecording(false)
    setError(null)
  }, [entry, open])

  // Why: suspend global shortcut dispatch while recording so the captured chord lands here instead of firing.
  useEffect(() => {
    if (!recording) {
      return
    }
    window.api.ui.setShortcutRecorderFocused(true)
    return () => window.api.ui.setShortcutRecorderFocused(false)
  }, [recording])

  const decoded = actionType === 'sendText' ? decodeCustomSendText(sendText) : null
  const bareWarnings = bindings
    .map((binding) => bareChordShadowWarning(binding, platform))
    .filter((warning): warning is string => warning !== null)

  const captureChord = (input: KeybindingInput): void => {
    const captured = keybindingFromInputForCustom(input, platform)
    if (!captured.ok) {
      setError(captured.error)
      return
    }
    setError(null)
    setRecording(false)
    setBindings((current) =>
      current.includes(captured.value) ? current : [...current, captured.value]
    )
    // Why: bare punctuation is the CJK-remap case — position matching is required there (#9338).
    if (BARE_PUNCTUATION_CHORD_PATTERN.test(captured.value)) {
      setMatchPhysicalKey(true)
    }
  }

  const handleRecorderKeyDown = (event: React.KeyboardEvent<HTMLButtonElement>): void => {
    if (!recording) {
      return
    }
    event.preventDefault()
    event.stopPropagation()
    if (event.key === 'Escape') {
      setRecording(false)
      return
    }
    // A modifier press never captures on its own; custom chords have no double-tap form.
    if (modifierFromKeyEvent(event.code, event.key) !== null) {
      return
    }
    captureChord({
      key: event.key,
      code: event.code,
      alt: event.altKey,
      meta: event.metaKey,
      control: event.ctrlKey,
      shift: event.shiftKey
    })
  }

  const save = async (): Promise<void> => {
    const trimmedTitle = title.trim()
    if (!trimmedTitle) {
      setError(
        translate('auto.components.settings.CustomShortcutEditor.titleRequired', 'Enter a title.')
      )
      return
    }
    if (bindings.length === 0) {
      setError(
        translate(
          'auto.components.settings.CustomShortcutEditor.chordRequired',
          'Record at least one shortcut.'
        )
      )
      return
    }
    if (actionType === 'sendText' && decoded && !decoded.ok) {
      setError(decoded.error)
      return
    }
    if (actionType === 'runQuickCommand' && !quickCommandId) {
      setError(
        translate(
          'auto.components.settings.CustomShortcutEditor.quickCommandRequired',
          'Pick a quick command.'
        )
      )
      return
    }
    const candidate: CustomKeybinding = {
      id: entry?.id ?? generateCustomKeybindingId(),
      title: trimmedTitle,
      action:
        actionType === 'sendText'
          ? { type: 'sendText', text: sendText }
          : { type: 'runQuickCommand', quickCommandId },
      bindings,
      ...(matchPhysicalKey ? { matchPhysicalKey: true } : {}),
      ...(entry?.when !== undefined ? { when: entry.when } : {})
    }
    // Why: pre-check mirrors the main-process write block so conflicts surface with named counterparts before the IPC round-trip.
    const candidateList = [
      ...customKeybindings.filter((existing) => existing.id !== candidate.id),
      candidate
    ]
    const blocking = findKeybindingConflicts(platform, keybindings, {}, candidateList).find(
      (conflict) => conflict.actionIds.includes(candidate.id)
    )
    if (blocking) {
      const counterparts = blocking.actionIds
        .filter((id) => id !== candidate.id)
        .map((id) => resolveKeybindingTitle(id, candidateList))
        .join(', ')
      setError(
        translate(
          'auto.components.settings.CustomShortcutEditor.conflict',
          '{{value0}} conflicts with {{value1}}.',
          { value0: formatKeybinding(blocking.binding, platform).join(''), value1: counterparts }
        )
      )
      return
    }
    try {
      await upsertCustomKeybinding(candidate)
      onClose()
    } catch (saveError) {
      setError(
        saveError instanceof Error
          ? saveError.message
          : translate(
              'auto.components.settings.CustomShortcutEditor.saveFailed',
              'Failed to save custom shortcut.'
            )
      )
    }
  }

  return (
    <Dialog open={open} onOpenChange={(nextOpen) => (nextOpen ? undefined : onClose())}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>
            {entry
              ? translate(
                  'auto.components.settings.CustomShortcutEditor.editTitle',
                  'Edit Custom Shortcut'
                )
              : translate(
                  'auto.components.settings.CustomShortcutEditor.addTitle',
                  'Add Custom Shortcut'
                )}
          </DialogTitle>
        </DialogHeader>
        <div className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="custom-shortcut-title">
              {translate('auto.components.settings.CustomShortcutEditor.titleLabel', 'Title')}
            </Label>
            <Input
              id="custom-shortcut-title"
              value={title}
              onChange={(event) => setTitle(event.target.value)}
              maxLength={64}
            />
          </div>

          <div className="space-y-1.5">
            <Label>
              {translate('auto.components.settings.CustomShortcutEditor.shortcutLabel', 'Shortcut')}
            </Label>
            <div className="flex flex-wrap items-center gap-1.5">
              {bindings.map((binding, index) => (
                <button
                  key={binding}
                  type="button"
                  className="flex items-center gap-1 rounded-md border px-2 py-1 text-xs hover:bg-accent"
                  aria-label={translate(
                    'auto.components.settings.CustomShortcutEditor.removeBinding',
                    'Remove shortcut {{value0}}',
                    { value0: formatKeybinding(binding, platform).join('') }
                  )}
                  onClick={() =>
                    setBindings((current) => current.filter((_, other) => other !== index))
                  }
                >
                  <ShortcutKeyCombo keys={formatKeybinding(binding, platform)} />
                  <span aria-hidden>×</span>
                </button>
              ))}
              <button
                type="button"
                data-shortcut-recorder=""
                data-shortcut-recorder-active={recording ? '' : undefined}
                aria-pressed={recording}
                className="min-h-7 rounded-md border border-dashed px-2 py-1 text-xs text-muted-foreground hover:bg-accent"
                onClick={() => setRecording(true)}
                onKeyDown={handleRecorderKeyDown}
              >
                {recording
                  ? translate(
                      'auto.components.settings.CustomShortcutEditor.pressKeys',
                      'Press keys…'
                    )
                  : translate(
                      'auto.components.settings.CustomShortcutEditor.recordShortcut',
                      'Record shortcut'
                    )}
              </button>
            </div>
            {bareWarnings.map((warning) => (
              <p key={warning} className="text-xs text-muted-foreground" role="alert">
                ⚠ {warning}
              </p>
            ))}
          </div>

          <CustomShortcutActionFields
            actionType={actionType}
            onActionTypeChange={setActionType}
            sendText={sendText}
            onSendTextChange={setSendText}
            decoded={decoded}
            quickCommandId={quickCommandId}
            onQuickCommandIdChange={setQuickCommandId}
            terminalQuickCommands={terminalQuickCommands}
          />

          <div className="flex items-start gap-2">
            <Checkbox
              id="custom-shortcut-physical"
              checked={matchPhysicalKey}
              onCheckedChange={(checked) => setMatchPhysicalKey(checked === true)}
            />
            <div className="space-y-0.5">
              <Label htmlFor="custom-shortcut-physical">
                {translate(
                  'auto.components.settings.CustomShortcutEditor.matchPhysicalKey',
                  'Match by key position'
                )}
              </Label>
              <p className="text-xs text-muted-foreground">
                {translate(
                  'auto.components.settings.CustomShortcutEditor.matchPhysicalKeyHelp',
                  'Required for remapping keys while a CJK input method is active.'
                )}
              </p>
            </div>
          </div>

          {error ? (
            <p className="text-xs text-destructive" role="alert">
              {error}
            </p>
          ) : null}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {translate('auto.components.settings.CustomShortcutEditor.cancel', 'Cancel')}
          </Button>
          <Button onClick={() => void save()}>
            {translate('auto.components.settings.CustomShortcutEditor.save', 'Save')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
