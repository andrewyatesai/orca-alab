import React, { useCallback, useMemo, useState } from 'react'
import { ArrowRight, Check, ChevronsUpDown, Star, Terminal, Wrench } from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  Command,
  CommandEmpty,
  CommandInput,
  CommandItem,
  CommandList
} from '@/components/ui/command'
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger
} from '@/components/ui/context-menu'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { AgentIcon, type AgentCatalogEntry } from '@/lib/agent-catalog'
import {
  agentPickerBlankTerminalMatches,
  getAgentPickerCommandValue,
  searchAgentPickerCustomProfiles,
  searchAgentPickerEntries
} from '@/lib/agent-picker-search'
import { cn } from '@/lib/utils'
import type { CustomAgentProfile, TuiAgent } from '../../../../shared/types'
import {
  createAgentComboboxCommandState,
  resolveAgentComboboxCommandState,
  updateAgentComboboxCommandValue
} from './agent-combobox-command-state'
import { translate } from '@/i18n/i18n'

type DefaultAgentPreference = TuiAgent | 'blank' | { kind: 'custom'; id: string } | null

/** Selection emitted by the combobox. The picker treats blank, built-in, and
 *  custom-profile rows as a tri-state so callers don't have to translate
 *  between two parallel value/onValueChange channels. */
export type AgentSelection =
  | { kind: 'blank' }
  | { kind: 'builtin'; agent: TuiAgent }
  | { kind: 'custom'; profile: CustomAgentProfile }

type AgentComboboxProps = {
  agents: AgentCatalogEntry[]
  customAgents?: CustomAgentProfile[]
  value: AgentSelection
  onValueChange: (selection: AgentSelection) => void
  onValueSelected?: (selection: AgentSelection) => void
  onOpenManageAgents?: () => void
  /** Current saved default agent preference. Used to render a subtle "default"
   *  indicator in the list and to tell which right-click menu item is the
   *  currently-applied choice. */
  defaultAgent?: DefaultAgentPreference
  /** Optional handler for right-click "Set as default" action. When provided,
   *  each list item (including Blank Terminal) gets a context menu. */
  onSetDefault?: (selection: DefaultAgentPreference) => void
  triggerClassName?: string
  /** When set, pressing Enter on the closed combobox trigger invokes this
   *  instead of opening the popover — lets the parent form treat the Agent
   *  field as the last keyboard-submit step. */
  onTriggerEnter?: () => void
  allowNarrowTrigger?: boolean
}

const BLANK_VALUE = '__none__'
// Why: stable default so the memoized custom-profile search isn't re-run every render.
const NO_CUSTOM_AGENTS: CustomAgentProfile[] = []
const TRIGGER_MIN_WIDTH_CLASS = '!min-w-[260px]'

type ItemRenderArgs = {
  key: string
  itemValue: string
  isChecked: boolean
  isDefault: boolean
  onSelect: () => void
  onSetDefault?: () => void
  icon: React.ReactNode
  label: string
}

function renderItem({
  key,
  itemValue,
  isChecked,
  isDefault,
  onSelect,
  onSetDefault,
  icon,
  label
}: ItemRenderArgs): React.ReactNode {
  const row = (
    <CommandItem
      key={key}
      value={itemValue}
      onSelect={onSelect}
      className="items-center gap-2 px-3 py-1.5"
    >
      <Check className={cn('size-4 text-foreground', isChecked ? 'opacity-100' : 'opacity-0')} />
      <span className="inline-flex min-w-0 flex-1 items-center gap-1.5">
        {icon}
        <span className="truncate">{label}</span>
      </span>
    </CommandItem>
  )
  if (!onSetDefault) {
    return row
  }
  return (
    // Why: z-[70] sits above PopoverContent's z-[60] so the right-click menu
    // renders in front of the still-open combobox popover instead of behind it.
    <ContextMenu key={key}>
      <ContextMenuTrigger asChild>{row}</ContextMenuTrigger>
      <ContextMenuContent className="z-[70]">
        <ContextMenuItem onSelect={onSetDefault} disabled={isDefault}>
          <Star className="size-3.5" />
          {isDefault
            ? translate('auto.components.agent.AgentCombobox.1b0d6965fa', 'Current default')
            : translate('auto.components.agent.AgentCombobox.9c6b59fe58', 'Set as default')}
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  )
}

export default function AgentCombobox({
  agents,
  customAgents = NO_CUSTOM_AGENTS,
  value,
  onValueChange,
  onValueSelected,
  onOpenManageAgents,
  defaultAgent,
  onSetDefault,
  triggerClassName,
  onTriggerEnter,
  allowNarrowTrigger = false
}: AgentComboboxProps): React.JSX.Element {
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  // Why: controlled cmdk selection so hovering the footer (which lives outside
  // the cmdk tree) can clear the list's highlighted item — otherwise cmdk keeps
  // the last-hovered agent visually selected while the mouse is on the footer.
  const [commandState, setCommandState] = useState(() => createAgentComboboxCommandState(''))
  const triggerRef = React.useRef<HTMLButtonElement | null>(null)
  const inputRef = React.useRef<HTMLInputElement | null>(null)
  const focusFrameRef = React.useRef<number | null>(null)

  const selectedBuiltin = useMemo<AgentCatalogEntry | null>(
    () =>
      value.kind === 'builtin' ? (agents.find((agent) => agent.id === value.agent) ?? null) : null,
    [agents, value]
  )
  const selectedCustom = useMemo<CustomAgentProfile | null>(
    () => (value.kind === 'custom' ? value.profile : null),
    [value]
  )
  // Why: cmdk item value of the current tri-state selection; custom rows key as
  // `custom:<id>` so command-state seeding/highlighting stays string-based.
  const valueCommandKey =
    value.kind === 'builtin'
      ? value.agent
      : value.kind === 'custom'
        ? `custom:${value.profile.id}`
        : BLANK_VALUE
  const filteredAgents = useMemo(() => searchAgentPickerEntries(agents, query), [agents, query])
  const filteredCustomAgents = useMemo(
    () => searchAgentPickerCustomProfiles(customAgents, query),
    [customAgents, query]
  )
  const blankMatchesQuery = useMemo(() => agentPickerBlankTerminalMatches(query), [query])
  const activeCommandValue = getAgentPickerCommandValue({
    blankValue: BLANK_VALUE,
    blankMatchesQuery,
    currentValue: valueCommandKey,
    filteredAgents,
    filteredCustomAgentValues: filteredCustomAgents.map((profile) => `custom:${profile.id}`),
    rawQuery: query
  })
  const resolvedCommandState = resolveAgentComboboxCommandState(
    commandState,
    open,
    activeCommandValue
  )
  if (resolvedCommandState !== commandState) {
    // Why: cmdk highlights should follow query/result changes before paint,
    // while manual hover selection remains intact until the active candidate changes.
    setCommandState(resolvedCommandState)
  }
  const commandValue = resolvedCommandState.commandValue

  const cancelFocusFrame = useCallback((): void => {
    if (focusFrameRef.current !== null) {
      cancelAnimationFrame(focusFrameRef.current)
      focusFrameRef.current = null
    }
  }, [])

  const setInputNode = useCallback(
    (node: HTMLInputElement | null): void => {
      if (node === null) {
        cancelFocusFrame()
      }
      inputRef.current = node
    },
    [cancelFocusFrame]
  )

  const setCommandValue = useCallback((nextCommandValue: string): void => {
    setCommandState((current) => updateAgentComboboxCommandValue(current, nextCommandValue))
  }, [])

  const focusSearchInput = useCallback(() => {
    cancelFocusFrame()
    focusFrameRef.current = requestAnimationFrame(() => {
      focusFrameRef.current = null
      const searchInput = inputRef.current
      if (!searchInput) {
        return
      }
      searchInput.focus()
      // Why: when a printable keydown on the trigger seeded the query, the user
      // expects the next keystroke to append to what they typed — not replace
      // it — so drop the caret at the end instead of selecting all.
      const end = searchInput.value.length
      searchInput.setSelectionRange(end, end)
    })
  }, [cancelFocusFrame])

  const handleOpenChange = useCallback(
    (nextOpen: boolean) => {
      setOpen(nextOpen)
      if (nextOpen) {
        setCommandState(createAgentComboboxCommandState(valueCommandKey))
        return
      }
      cancelFocusFrame()
      setQuery('')
    },
    [cancelFocusFrame, valueCommandKey]
  )

  const handleSelect = useCallback(
    (next: AgentSelection) => {
      onValueChange(next)
      setOpen(false)
      setQuery('')
      onValueSelected?.(next)
    },
    [onValueChange, onValueSelected]
  )

  // Why: mirror RepoCombobox's trigger-keydown handling — the button-style
  // trigger treats the current value as a confirmed selection. Plain focus does
  // not open the dropdown. Only explicit intent opens: Arrow keys open without
  // filtering; a printable non-whitespace char opens AND seeds the search
  // query (treating the keystroke as the start of a new search).
  const handleTriggerKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLButtonElement>) => {
      if (open) {
        return
      }
      if (
        event.key === 'Enter' &&
        onTriggerEnter &&
        !event.shiftKey &&
        !event.metaKey &&
        !event.ctrlKey &&
        !event.altKey
      ) {
        event.preventDefault()
        onTriggerEnter()
        return
      }
      if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault()
        setCommandState(createAgentComboboxCommandState(valueCommandKey))
        setOpen(true)
        return
      }
      if (event.metaKey || event.ctrlKey || event.altKey) {
        return
      }
      if (event.key.length === 1 && /\S/.test(event.key)) {
        event.preventDefault()
        setCommandState(createAgentComboboxCommandState(valueCommandKey))
        setQuery(event.key)
        setOpen(true)
      }
    },
    [open, onTriggerEnter, valueCommandKey]
  )

  return (
    <div className="flex w-full items-center">
      <Popover open={open} onOpenChange={handleOpenChange}>
        <PopoverTrigger asChild>
          <Button
            ref={triggerRef}
            type="button"
            variant="outline"
            role="combobox"
            aria-expanded={open}
            onKeyDown={handleTriggerKeyDown}
            className={cn(
              // Why: callers sometimes pass `min-w-0` for grid layouts, but
              // the compact trigger still needs room for "GitHub Copilot".
              'h-8 justify-between px-3 text-xs font-normal',
              triggerClassName,
              !allowNarrowTrigger && TRIGGER_MIN_WIDTH_CLASS
            )}
            data-agent-combobox-root="true"
          >
            {selectedBuiltin ? (
              <span className="inline-flex min-w-0 flex-1 items-center gap-1.5">
                <AgentIcon agent={selectedBuiltin.id} />
                <span className="truncate">{selectedBuiltin.label}</span>
              </span>
            ) : selectedCustom ? (
              <span className="inline-flex min-w-0 flex-1 items-center gap-1.5">
                <AgentIcon agent={selectedCustom.baseAgent} />
                <span className="truncate">{selectedCustom.label}</span>
              </span>
            ) : (
              <span className="inline-flex min-w-0 flex-1 items-center gap-1.5">
                <Terminal className="size-3.5" />
                <span className="truncate">
                  {translate('auto.components.agent.AgentCombobox.986f946354', 'Blank Terminal')}
                </span>
              </span>
            )}
            <ChevronsUpDown className="size-3.5 opacity-50" />
          </Button>
        </PopoverTrigger>
        <PopoverContent
          align="start"
          className={cn(
            'w-[var(--radix-popover-trigger-width)] p-0',
            !allowNarrowTrigger && 'min-w-[18rem]'
          )}
          data-agent-combobox-root="true"
          onOpenAutoFocus={(event) => {
            event.preventDefault()
            focusSearchInput()
          }}
        >
          <Command shouldFilter={false} value={commandValue} onValueChange={setCommandValue}>
            <CommandInput
              ref={setInputNode}
              placeholder={translate(
                'auto.components.agent.AgentCombobox.48c6a5a9b4',
                'Search agents...'
              )}
              value={query}
              onValueChange={setQuery}
            />
            <CommandList>
              <CommandEmpty>
                {translate(
                  'auto.components.agent.AgentCombobox.579c768bde',
                  'No agents match your search.'
                )}
              </CommandEmpty>
              {blankMatchesQuery
                ? renderItem({
                    key: BLANK_VALUE,
                    itemValue: BLANK_VALUE,
                    isChecked: value.kind === 'blank',
                    isDefault: defaultAgent === 'blank',
                    onSelect: () => handleSelect({ kind: 'blank' }),
                    onSetDefault: onSetDefault ? () => onSetDefault('blank') : undefined,
                    icon: <Terminal className="size-3.5" />,
                    label: translate(
                      'auto.components.agent.AgentCombobox.986f946354',
                      'Blank Terminal'
                    )
                  })
                : null}
              {filteredAgents.map((agent) =>
                renderItem({
                  key: agent.id,
                  itemValue: agent.id,
                  isChecked: value.kind === 'builtin' && value.agent === agent.id,
                  isDefault: defaultAgent === agent.id,
                  onSelect: () => handleSelect({ kind: 'builtin', agent: agent.id }),
                  onSetDefault: onSetDefault ? () => onSetDefault(agent.id) : undefined,
                  icon: <AgentIcon agent={agent.id} />,
                  label: agent.label
                })
              )}
              {filteredCustomAgents.map((profile) => {
                const key = `custom:${profile.id}`
                const isCustomDefault =
                  typeof defaultAgent === 'object' &&
                  defaultAgent !== null &&
                  defaultAgent.kind === 'custom' &&
                  defaultAgent.id === profile.id
                return renderItem({
                  key,
                  itemValue: key,
                  isChecked: value.kind === 'custom' && value.profile.id === profile.id,
                  isDefault: isCustomDefault,
                  onSelect: () => handleSelect({ kind: 'custom', profile }),
                  onSetDefault: onSetDefault
                    ? () => onSetDefault({ kind: 'custom', id: profile.id })
                    : undefined,
                  // Why: custom profiles inherit the base agent's icon so the
                  // picker visually groups variants of the same CLI together.
                  // The Wrench overlay disambiguates that this is a user-
                  // configured variant rather than a stock entry.
                  icon: (
                    <span className="relative inline-flex">
                      <AgentIcon agent={profile.baseAgent} />
                      <Wrench
                        className="absolute -right-1 -bottom-1 size-2 rounded-sm bg-background p-[1px] text-muted-foreground"
                        aria-hidden
                      />
                    </span>
                  ),
                  label: profile.label
                })
              })}
            </CommandList>
            {onOpenManageAgents ? (
              <div className="border-t border-border">
                <Button
                  type="button"
                  variant="ghost"
                  onClick={onOpenManageAgents}
                  onMouseDown={(event) => event.preventDefault()}
                  onMouseEnter={() => setCommandValue('')}
                  className="h-9 w-full justify-start rounded-none px-3 text-xs font-normal text-muted-foreground"
                >
                  {translate('auto.components.agent.AgentCombobox.19522e25ee', 'Manage agents')}
                  <ArrowRight className="ml-auto size-3" />
                </Button>
              </div>
            ) : null}
          </Command>
        </PopoverContent>
      </Popover>
    </div>
  )
}
