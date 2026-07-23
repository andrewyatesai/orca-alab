import { Plus, Trash2 } from 'lucide-react'
import { getAgentCatalog } from '@/lib/agent-catalog'
import type { TuiAgent } from '../../../../shared/types'
import { Button } from '../ui/button'
import { Input } from '../ui/input'
import { translate } from '@/i18n/i18n'
import { newCustomAgentEnvPair, type CustomAgentDraftRow } from './custom-agent-profile-draft'

type CustomAgentProfileEditorProps = {
  draft: CustomAgentDraftRow
  onDraftChange: (patch: Partial<CustomAgentDraftRow>) => void
  onSave: () => void
}

/** Expanded edit panel for one custom-agent row: base agent, command, env vars. */
export function CustomAgentProfileEditor({
  draft,
  onDraftChange,
  onSave
}: CustomAgentProfileEditorProps): React.JSX.Element {
  return (
    <div className="space-y-3 border-t border-border/40 px-4 py-3">
      <label className="block">
        <span className="text-xs text-muted-foreground">
          {translate('auto.components.settings.CustomAgentsSection.db6ffcf46c', 'Base agent')}
        </span>
        <select
          value={draft.baseAgent}
          onChange={(e) => onDraftChange({ baseAgent: e.target.value as TuiAgent })}
          className="mt-1 h-8 w-full rounded-md border border-input bg-background px-2 text-xs"
        >
          {getAgentCatalog().map((opt) => (
            <option key={opt.id} value={opt.id}>
              {opt.label}
            </option>
          ))}
        </select>
        <p className="mt-1 text-[11px] text-muted-foreground">
          {translate(
            'auto.components.settings.CustomAgentsSection.6d6b6383c2',
            'Inherits prompt-injection mode, icon, and trust preflight from this built-in agent.'
          )}
        </p>
      </label>

      <label className="block">
        <span className="text-xs text-muted-foreground">
          {translate('auto.components.settings.CustomAgentsSection.9c9f93700f', 'Command')}
        </span>
        <Input
          value={draft.command}
          onChange={(e) => onDraftChange({ command: e.target.value })}
          placeholder={translate(
            'auto.components.settings.CustomAgentProfileEditor.520ba5f307',
            'claude'
          )}
          spellCheck={false}
          className="mt-1 h-7 font-mono text-xs"
        />
        <p className="mt-1 text-[11px] text-muted-foreground">
          {translate(
            'auto.components.settings.CustomAgentsSection.4e2b76d091',
            'Shell command used to launch the agent. Env vars below are prepended as a shell prefix (POSIX quoting) before this command.'
          )}
        </p>
      </label>

      <div className="space-y-1">
        <span className="text-xs text-muted-foreground">
          {translate(
            'auto.components.settings.CustomAgentsSection.6d18282737',
            'Environment variables'
          )}
        </span>
        <div className="space-y-1.5">
          {draft.envPairs.map((pair, idx) => (
            <div key={pair.id} className="flex items-center gap-2">
              <Input
                value={pair.key}
                onChange={(e) => {
                  const envPairs = [...draft.envPairs]
                  envPairs[idx] = { ...pair, key: e.target.value }
                  onDraftChange({ envPairs })
                }}
                placeholder={translate(
                  'auto.components.settings.CustomAgentProfileEditor.0a09ab825e',
                  'KEY'
                )}
                spellCheck={false}
                className="h-7 flex-1 font-mono text-xs"
              />
              <span className="text-xs text-muted-foreground">=</span>
              <Input
                value={pair.value}
                onChange={(e) => {
                  const envPairs = [...draft.envPairs]
                  envPairs[idx] = { ...pair, value: e.target.value }
                  onDraftChange({ envPairs })
                }}
                placeholder={translate(
                  'auto.components.settings.CustomAgentProfileEditor.6c0fb2434a',
                  'value'
                )}
                spellCheck={false}
                className="h-7 flex-[2] font-mono text-xs"
              />
              <Button
                type="button"
                variant="ghost"
                size="icon"
                onClick={() => {
                  const envPairs = draft.envPairs.filter((_, i) => i !== idx)
                  onDraftChange({
                    envPairs: envPairs.length === 0 ? [newCustomAgentEnvPair()] : envPairs
                  })
                }}
                title={translate(
                  'auto.components.settings.CustomAgentsSection.e1d70fd5a1',
                  'Remove env var'
                )}
                className="size-7 text-muted-foreground hover:text-foreground"
              >
                <Trash2 className="size-3" />
              </Button>
            </div>
          ))}
        </div>
        <Button
          type="button"
          variant="ghost"
          size="xs"
          onClick={() => onDraftChange({ envPairs: [...draft.envPairs, newCustomAgentEnvPair()] })}
          className="h-7 text-xs text-muted-foreground hover:text-foreground"
        >
          <Plus className="size-3" />{' '}
          {translate('auto.components.settings.CustomAgentsSection.0f12fd331d', 'Add variable')}
        </Button>
      </div>

      <div className="flex justify-end">
        <Button
          type="button"
          variant="default"
          size="sm"
          onClick={onSave}
          disabled={!draft.label.trim() || !draft.command.trim()}
          className="h-7 text-xs"
        >
          {translate('auto.components.settings.CustomAgentsSection.fb32b3444c', 'Save')}
        </Button>
      </div>
    </div>
  )
}
