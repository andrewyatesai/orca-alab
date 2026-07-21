import { useEffect, useRef, useState } from 'react'
import { Plus, Trash2, Wrench } from 'lucide-react'
import type { CustomAgentProfile, TuiAgent } from '../../../../shared/types'
import { getAgentCatalog, AgentIcon } from '@/lib/agent-catalog'
import { Button } from '../ui/button'
import { Input } from '../ui/input'
import { cn } from '@/lib/utils'
import { translate } from '@/i18n/i18n'
import { CustomAgentProfileEditor } from './CustomAgentProfileEditor'
import {
  customAgentDraftToProfile,
  customAgentProfileToDraft,
  newCustomAgentDraftFor,
  type CustomAgentDraftRow
} from './custom-agent-profile-draft'

type CustomAgentsSectionProps = {
  customAgents: CustomAgentProfile[]
  onChange: (next: CustomAgentProfile[]) => void
  /** Default-agent control — surfaces a "Set as default" affordance per
   *  profile so the user can wire a custom agent into the auto-pick flow. */
  defaultCustomAgentId: string | null
  onSetDefault: (id: string) => void
  /** Why: shell-quoting for env values is POSIX-shaped. Surface a hint on
   *  Windows so users don't paste an env map and silently get a no-op. */
  isWindows: boolean
}

export function CustomAgentsSection({
  customAgents,
  onChange,
  defaultCustomAgentId,
  onSetDefault,
  isWindows
}: CustomAgentsSectionProps): React.JSX.Element {
  // Why: edits live in local draft state per-row so users can experiment with
  // env keys/values without each keystroke writing to global settings (and
  // racing with re-renders that would scroll the field out from under them).
  // Drafts commit on blur or Save click.
  const [drafts, setDrafts] = useState<CustomAgentDraftRow[]>(() =>
    customAgents.map(customAgentProfileToDraft)
  )
  const [editingId, setEditingId] = useState<string | null>(null)
  const lastExternalRef = useRef<CustomAgentProfile[]>(customAgents)

  useEffect(() => {
    // Why: external settings updates (e.g. settings sync, undo) should
    // refresh local drafts unless the user is mid-edit. Reference-equality
    // check skips the trivial case where this component itself wrote.
    if (lastExternalRef.current === customAgents) {
      return
    }
    lastExternalRef.current = customAgents
    if (editingId) {
      // Don't yank focus; merge non-edited rows from external state.
      setDrafts((prev) => {
        const externalById = new Map(customAgents.map((p) => [p.id, p]))
        const next = prev
          .filter((d) => d.id === editingId || externalById.has(d.id))
          .map((d) => {
            if (d.id === editingId) {
              return d
            }
            const ext = externalById.get(d.id)
            return ext ? customAgentProfileToDraft(ext) : d
          })
        // Append newly-external rows we didn't have locally.
        for (const ext of customAgents) {
          if (!next.find((d) => d.id === ext.id)) {
            next.push(customAgentProfileToDraft(ext))
          }
        }
        return next
      })
      return
    }
    setDrafts(customAgents.map(customAgentProfileToDraft))
  }, [customAgents, editingId])

  const commit = (next: CustomAgentDraftRow[]): void => {
    const profiles = next
      .map(customAgentDraftToProfile)
      .filter((p): p is CustomAgentProfile => p !== null)
    lastExternalRef.current = profiles
    onChange(profiles)
  }

  const updateDraft = (id: string, patch: Partial<CustomAgentDraftRow>): void => {
    setDrafts((prev) => prev.map((d) => (d.id === id ? { ...d, ...patch } : d)))
  }

  const addProfile = (baseAgent: TuiAgent): void => {
    const draft = newCustomAgentDraftFor(baseAgent)
    setDrafts((prev) => [...prev, draft])
    setEditingId(draft.id)
  }

  const removeProfile = (id: string): void => {
    const next = drafts.filter((d) => d.id !== id)
    setDrafts(next)
    if (editingId === id) {
      setEditingId(null)
    }
    commit(next)
  }

  const finishEdit = (id: string): void => {
    setEditingId((cur) => (cur === id ? null : cur))
    commit(drafts)
  }

  return (
    <section className="space-y-3">
      <div className="flex items-center justify-between gap-3">
        <div className="space-y-1">
          <h3 className="text-sm font-semibold">
            {translate('auto.components.settings.CustomAgentsSection.916fc48436', 'Custom Agents')}
          </h3>
          <p className="text-xs text-muted-foreground">
            {translate(
              'auto.components.settings.CustomAgentsSection.576a6eedaa',
              'Named variants of a built-in agent with their own command and env vars'
            )}
            {isWindows
              ? ` ${translate(
                  'auto.components.settings.CustomAgentsSection.cdcae0f619',
                  '(env prefix uses POSIX shell quoting; on Windows wrap with `cmd /c …` if needed)'
                )}`
              : ''}
            .
          </p>
        </div>
        <AddProfileButton onAdd={addProfile} />
      </div>

      {drafts.length === 0 ? (
        <div className="rounded-xl border border-dashed border-border/50 px-4 py-6 text-center text-xs text-muted-foreground">
          {translate(
            'auto.components.settings.CustomAgentsSection.ae2e07a0bc',
            'No custom agents yet. Click'
          )}{' '}
          <span className="font-medium">
            {translate('auto.components.settings.CustomAgentsSection.f7ada76050', 'Add')}
          </span>{' '}
          {translate(
            'auto.components.settings.CustomAgentsSection.eee34f329b',
            'to define one — for example, a Claude profile pointed at a self-hosted endpoint via'
          )}{' '}
          <code className="rounded bg-muted/40 px-1 py-0.5 font-mono">
            {translate(
              'auto.components.settings.CustomAgentsSection.eeaceac120',
              'ANTHROPIC_BASE_URL'
            )}
          </code>
          .
        </div>
      ) : (
        <div className="space-y-2">
          {drafts.map((draft) => {
            const isEditing = editingId === draft.id
            const isDefault = defaultCustomAgentId === draft.id
            return (
              <div
                key={draft.id}
                className={cn(
                  'rounded-xl border bg-card/60 transition-all',
                  isEditing ? 'border-foreground/30' : 'border-border/60'
                )}
              >
                <div className="flex items-center gap-3 px-4 py-3">
                  <div className="relative flex size-8 shrink-0 items-center justify-center rounded-lg border border-border/50 bg-background/60">
                    <AgentIcon agent={draft.baseAgent} size={18} />
                    <Wrench
                      className="absolute -right-1 -bottom-1 size-3 rounded-sm bg-background p-[1px] text-muted-foreground"
                      aria-hidden
                    />
                  </div>
                  <div className="min-w-0 flex-1">
                    {isEditing ? (
                      <Input
                        value={draft.label}
                        onChange={(e) => updateDraft(draft.id, { label: e.target.value })}
                        placeholder={translate(
                          'auto.components.settings.CustomAgentsSection.37ce37fd95',
                          'e.g. Claude (zai)'
                        )}
                        className="h-7 text-sm"
                      />
                    ) : (
                      <div className="truncate text-sm font-semibold leading-none">
                        {draft.label || (
                          <span className="text-muted-foreground">
                            {translate(
                              'auto.components.settings.CustomAgentsSection.4ba5122112',
                              'Unnamed'
                            )}
                          </span>
                        )}
                      </div>
                    )}
                    <div className="mt-0.5 truncate font-mono text-[11px] text-muted-foreground">
                      {draft.command || (
                        <span className="italic">
                          {translate(
                            'auto.components.settings.CustomAgentsSection.8e6e786c9a',
                            'no command'
                          )}
                        </span>
                      )}
                    </div>
                  </div>
                  <div className="flex shrink-0 items-center gap-1">
                    {!isEditing && (
                      <button
                        type="button"
                        onClick={() => onSetDefault(draft.id)}
                        title={
                          isDefault
                            ? translate(
                                'auto.components.settings.CustomAgentsSection.f0e1d2c3b4',
                                'Default agent'
                              )
                            : translate(
                                'auto.components.settings.CustomAgentsSection.a5b6c7d8e9',
                                'Set as default'
                              )
                        }
                        className={cn(
                          'flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-xs font-medium transition-colors',
                          isDefault
                            ? 'bg-foreground/10 text-foreground ring-1 ring-foreground/20'
                            : 'text-muted-foreground hover:bg-muted/60 hover:text-foreground'
                        )}
                      >
                        {isDefault
                          ? translate(
                              'auto.components.settings.CustomAgentsSection.1a2b3c4d5e',
                              'Default'
                            )
                          : translate(
                              'auto.components.settings.CustomAgentsSection.6f7a8b9c0d',
                              'Set default'
                            )}
                      </button>
                    )}
                    <Button
                      type="button"
                      variant="ghost"
                      size="xs"
                      onClick={() => (isEditing ? finishEdit(draft.id) : setEditingId(draft.id))}
                      className="h-7 px-2 text-xs"
                    >
                      {isEditing
                        ? translate(
                            'auto.components.settings.CustomAgentsSection.8054ef4458',
                            'Done'
                          )
                        : translate(
                            'auto.components.settings.CustomAgentsSection.5d88f2e239',
                            'Edit'
                          )}
                    </Button>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      onClick={() => removeProfile(draft.id)}
                      title={translate(
                        'auto.components.settings.CustomAgentsSection.ca98ab9d85',
                        'Delete profile'
                      )}
                      className="size-7 text-muted-foreground hover:text-foreground"
                    >
                      <Trash2 className="size-3.5" />
                    </Button>
                  </div>
                </div>

                {isEditing && (
                  <CustomAgentProfileEditor
                    draft={draft}
                    onDraftChange={(patch) => updateDraft(draft.id, patch)}
                    onSave={() => finishEdit(draft.id)}
                  />
                )}
              </div>
            )
          })}
        </div>
      )}
    </section>
  )
}

function AddProfileButton({ onAdd }: { onAdd: (baseAgent: TuiAgent) => void }): React.JSX.Element {
  const [open, setOpen] = useState(false)
  return (
    <div className="relative">
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={() => setOpen((v) => !v)}
        className="h-8 gap-1.5 text-xs"
      >
        <Plus className="size-3.5" />
        {translate('auto.components.settings.CustomAgentsSection.f7ada76050', 'Add')}
      </Button>
      {open && (
        <div
          // Why: lightweight inline picker to choose the base agent up front
          // (so users don't open an Edit row pre-filled with the wrong one).
          // A full shadcn Popover here would pull more focus management than
          // necessary for a one-click base-agent select.
          className="absolute right-0 top-9 z-30 max-h-72 w-56 overflow-y-auto scrollbar-sleek rounded-lg border border-border/60 bg-popover p-1 text-popover-foreground shadow-md"
          onMouseLeave={() => setOpen(false)}
        >
          {getAgentCatalog().map((agent) => (
            <button
              key={agent.id}
              type="button"
              onClick={() => {
                onAdd(agent.id)
                setOpen(false)
              }}
              className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs hover:bg-muted/60"
            >
              <AgentIcon agent={agent.id} size={14} />
              <span>
                {translate('auto.components.settings.CustomAgentsSection.b4903769af', 'Based on')}{' '}
                {agent.label}
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  )
}
