import React from 'react'
import type { CustomSendTextDecodeResult } from '../../../../shared/custom-keybindings'
import type { TerminalQuickCommand } from '../../../../shared/types'
import { Input } from '../ui/input'
import { Label } from '../ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../ui/select'
import { ToggleGroup, ToggleGroupItem } from '../ui/toggle-group'
import { translate } from '@/i18n/i18n'

export type CustomShortcutActionType = 'sendText' | 'runQuickCommand'

type CustomShortcutActionFieldsProps = {
  actionType: CustomShortcutActionType
  onActionTypeChange: (actionType: CustomShortcutActionType) => void
  sendText: string
  onSendTextChange: (text: string) => void
  decoded: CustomSendTextDecodeResult | null
  quickCommandId: string
  onQuickCommandIdChange: (id: string) => void
  /** Already filtered to terminal-command quick commands (no agent prompts). */
  terminalQuickCommands: readonly TerminalQuickCommand[]
}

function toHexPreview(text: string): string {
  return Array.from(new TextEncoder().encode(text))
    .map((byte) => byte.toString(16).padStart(2, '0'))
    .join(' ')
}

/** Action chooser + per-action fields for the custom-shortcut editor dialog. */
export function CustomShortcutActionFields({
  actionType,
  onActionTypeChange,
  sendText,
  onSendTextChange,
  decoded,
  quickCommandId,
  onQuickCommandIdChange,
  terminalQuickCommands
}: CustomShortcutActionFieldsProps): React.JSX.Element {
  return (
    <>
      <div className="space-y-1.5">
        <Label>
          {translate('auto.components.settings.CustomShortcutEditor.actionLabel', 'Action')}
        </Label>
        <ToggleGroup
          type="single"
          value={actionType}
          onValueChange={(value) => {
            if (value === 'sendText' || value === 'runQuickCommand') {
              onActionTypeChange(value)
            }
          }}
        >
          <ToggleGroupItem value="sendText">
            {translate('auto.components.settings.CustomShortcutEditor.sendText', 'Send text')}
          </ToggleGroupItem>
          <ToggleGroupItem value="runQuickCommand">
            {translate(
              'auto.components.settings.CustomShortcutEditor.runQuickCommand',
              'Run quick command'
            )}
          </ToggleGroupItem>
        </ToggleGroup>
      </div>

      {actionType === 'sendText' ? (
        <div className="space-y-1.5">
          <Input
            aria-label={translate(
              'auto.components.settings.CustomShortcutEditor.sendTextLabel',
              'Text to send'
            )}
            className="font-mono"
            value={sendText}
            onChange={(event) => onSendTextChange(event.target.value)}
          />
          <p className="text-xs text-muted-foreground">
            {translate(
              'auto.components.settings.CustomShortcutEditor.escapesHelp',
              'Escapes: \\e \\xNN \\uNNNN \\n \\r \\t \\\\'
            )}
          </p>
          {/* Why: the live hex preview makes escape typos visible before saving. */}
          {sendText ? (
            decoded?.ok ? (
              <p className="font-mono text-xs text-muted-foreground" data-testid="hex-preview">
                {toHexPreview(decoded.text)}
              </p>
            ) : (
              <p className="text-xs text-destructive" role="alert">
                {decoded?.ok === false ? decoded.error : null}
              </p>
            )
          ) : null}
        </div>
      ) : (
        <div className="space-y-1.5">
          {terminalQuickCommands.length === 0 ? (
            <p className="text-xs text-muted-foreground">
              {translate(
                'auto.components.settings.CustomShortcutEditor.noQuickCommands',
                'No terminal quick commands yet. Create one in Settings → Quick Commands. Agent-prompt commands are not supported here.'
              )}
            </p>
          ) : (
            <Select value={quickCommandId} onValueChange={onQuickCommandIdChange}>
              <SelectTrigger
                aria-label={translate(
                  'auto.components.settings.CustomShortcutEditor.quickCommandLabel',
                  'Quick command'
                )}
              >
                <SelectValue
                  placeholder={translate(
                    'auto.components.settings.CustomShortcutEditor.pickQuickCommand',
                    'Pick a quick command'
                  )}
                />
              </SelectTrigger>
              <SelectContent>
                {terminalQuickCommands.map((command) => (
                  <SelectItem key={command.id} value={command.id}>
                    {command.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}
        </div>
      )}
    </>
  )
}
