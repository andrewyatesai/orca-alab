import React, { useCallback, useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { useAppStore } from '@/store'
import { findWorktreeById } from '@/store/slices/worktree-helpers'
import {
  CommandDialog,
  CommandInput,
  CommandList,
  CommandEmpty,
  CommandGroup,
  CommandItem
} from '@/components/ui/command'
import { useModalReturnFocus } from '@/hooks/useModalReturnFocus'
import { translate } from '@/i18n/i18n'
import {
  createProductionFederatedSearchController,
  jumpToFederatedResultInApp
} from '@/lib/federated-search/federated-search-production'
import { FEDERATED_QUERY_DEBOUNCE_MS } from '@/lib/federated-search/federated-search-model'
import type { FederatedResultGroup } from '@/lib/federated-search/federated-search-grouping'
import type { FederatedMatch } from '@/lib/federated-search/federated-search-model'

function sourceBadgeLabel(group: FederatedResultGroup): string {
  switch (group.source) {
    case 'hidden':
      return translate('auto.components.FederatedSearchPalette.badgeHidden', 'hidden')
    case 'parked':
      return translate('auto.components.FederatedSearchPalette.badgeParked', 'parked')
    case 'daemon-history':
      return translate('auto.components.FederatedSearchPalette.badgeHistory', 'history')
    case 'remote':
      return translate('auto.components.FederatedSearchPalette.badgeRemote', 'remote')
    case 'live':
      return '' // the default — no badge noise on ordinary panes
  }
}

function groupCountLabel(group: FederatedResultGroup): string {
  // Honesty markers: "N+" when the source truncated, "~" while stale.
  const base = `${group.total}${group.incomplete ? '+' : ''}`
  return group.stale ? `~${base}` : base
}

/** Global federated terminal search (Cmd/Ctrl+Shift+F in a terminal): one query
 *  across every pane orc knows about, grouped by pane, newest match first. */
export default function FederatedSearchPalette(): React.JSX.Element | null {
  const visible = useAppStore((s) => s.activeModal === 'federated-search')
  const modalData = useAppStore((s) => s.modalData)
  const closeModal = useAppStore((s) => s.closeModal)
  const worktreesByRepo = useAppStore((s) => s.worktreesByRepo)

  const [query, setQuery] = useState('')
  const [caseSensitive, setCaseSensitive] = useState(false)
  const [regexEnabled, setRegexEnabled] = useState(false)
  const { captureReturnFocus, skipReturnFocus } = useModalReturnFocus(visible)

  // One controller per mount; disposed with the component.
  const [controller] = useState(() => createProductionFederatedSearchController())
  useEffect(() => () => controller.dispose(), [controller])
  const snapshot = useSyncExternalStore(controller.subscribe, controller.snapshot)

  // Seed from the pane find bar's escape hatch; reset on each open.
  const [previousVisible, setPreviousVisible] = useState(visible)
  if (visible !== previousVisible) {
    setPreviousVisible(visible)
    if (visible) {
      const seed = typeof modalData.query === 'string' ? modalData.query : ''
      setQuery(seed)
    }
  }

  // 75ms debounce after the last keystroke (§1 liveness); each run bumps the
  // controller generation, cancelling in-flight source queries.
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  useEffect(() => {
    if (!visible) {
      return
    }
    if (debounceRef.current !== null) {
      clearTimeout(debounceRef.current)
    }
    debounceRef.current = setTimeout(() => {
      debounceRef.current = null
      controller.setQuery(query.trim(), { caseSensitive, isRegex: regexEnabled })
    }, FEDERATED_QUERY_DEBOUNCE_MS)
    return () => {
      if (debounceRef.current !== null) {
        clearTimeout(debounceRef.current)
        debounceRef.current = null
      }
    }
  }, [controller, visible, query, caseSensitive, regexEnabled])

  const handleOpenChange = useCallback(
    (open: boolean) => {
      if (!open) {
        // Esc/close cancels every in-flight source query (§1 liveness).
        controller.cancel()
        closeModal()
      }
    },
    [controller, closeModal]
  )

  const handleSelect = useCallback(
    (group: FederatedResultGroup, match: FederatedMatch) => {
      skipReturnFocus()
      controller.cancel()
      closeModal()
      void jumpToFederatedResultInApp(group, match, snapshot.query, snapshot.opts)
    },
    [controller, closeModal, skipReturnFocus, snapshot.query, snapshot.opts]
  )

  const handleCloseAutoFocus = useCallback((e: Event) => {
    // Why: prevent Radix from stealing focus to the trigger element.
    e.preventDefault()
  }, [])

  const handleOpenAutoFocus = useCallback(() => {
    captureReturnFocus()
  }, [captureReturnFocus])

  const worktreeName = (worktreeId: string | null): string | null => {
    if (!worktreeId) {
      return null
    }
    const worktree = findWorktreeById(worktreesByRepo, worktreeId)
    return worktree?.displayName ?? null
  }

  const groups = snapshot.groups.filter((g) => g.matches.length > 0 || g.overBudget)
  const totalShown = groups.reduce((sum, g) => sum + g.matches.length, 0)

  return (
    <CommandDialog
      open={visible}
      onOpenChange={handleOpenChange}
      shouldFilter={false}
      onOpenAutoFocus={handleOpenAutoFocus}
      onCloseAutoFocus={handleCloseAutoFocus}
      title={translate('auto.components.FederatedSearchPalette.title', 'Search all terminals')}
      description={translate(
        'auto.components.FederatedSearchPalette.description',
        'Search every terminal pane'
      )}
    >
      <div className="flex items-center gap-1 border-b border-border/60">
        <CommandInput
          wrapperClassName="flex-1 border-b-0"
          placeholder={translate(
            'auto.components.FederatedSearchPalette.placeholder',
            'Search all terminals...'
          )}
          value={query}
          onValueChange={setQuery}
        />
        <button
          type="button"
          aria-pressed={caseSensitive}
          onClick={() => setCaseSensitive((v) => !v)}
          className={`rounded px-2 py-1 mr-1 text-[11px] font-medium ${
            caseSensitive
              ? 'bg-accent text-accent-foreground'
              : 'text-muted-foreground hover:text-foreground'
          }`}
        >
          {translate('auto.components.FederatedSearchPalette.caseToggle', 'Aa')}
        </button>
        <button
          type="button"
          aria-pressed={regexEnabled}
          onClick={() => setRegexEnabled((v) => !v)}
          className={`rounded px-2 py-1 mr-2 text-[11px] font-medium ${
            regexEnabled
              ? 'bg-accent text-accent-foreground'
              : 'text-muted-foreground hover:text-foreground'
          }`}
        >
          {translate('auto.components.FederatedSearchPalette.regexToggle', '.*')}
        </button>
      </div>
      <CommandList className="p-2">
        {snapshot.query === '' ? (
          <div className="py-6 text-center text-sm text-muted-foreground">
            {translate(
              'auto.components.FederatedSearchPalette.emptyPrompt',
              'Type to search every terminal pane.'
            )}
          </div>
        ) : groups.length === 0 && !snapshot.pending ? (
          <CommandEmpty>
            {translate('auto.components.FederatedSearchPalette.noMatches', 'No matches.')}
          </CommandEmpty>
        ) : (
          groups.map((group) => {
            const badge = sourceBadgeLabel(group)
            const wtName = worktreeName(group.paneRef?.worktreeId ?? null)
            const heading = [
              wtName,
              group.paneRef?.title ??
                (group.paneRef
                  ? null
                  : translate(
                      'auto.components.FederatedSearchPalette.exitedSession',
                      'Exited session'
                    ))
            ]
              .filter(Boolean)
              .join(' / ')
            return (
              <CommandGroup
                key={group.key}
                heading={
                  <span className="flex items-center gap-2">
                    <span className="truncate">
                      {heading ||
                        translate(
                          'auto.components.FederatedSearchPalette.paneFallback',
                          'Terminal'
                        )}
                    </span>
                    {badge ? (
                      <span className="rounded-full border border-border/60 bg-muted/35 px-1.5 py-px text-[10px] text-muted-foreground">
                        {badge}
                      </span>
                    ) : null}
                    {group.hasDepthExtension ? (
                      <span className="rounded-full border border-border/60 bg-muted/35 px-1.5 py-px text-[10px] text-muted-foreground">
                        {translate('auto.components.FederatedSearchPalette.depthBadge', '+history')}
                      </span>
                    ) : null}
                    <span className="ml-auto text-[10px] text-muted-foreground">
                      {groupCountLabel(group)}
                    </span>
                  </span>
                }
              >
                {group.overBudget && group.matches.length === 0 ? (
                  <div className="px-3 py-1.5 text-xs text-muted-foreground">
                    {translate(
                      'auto.components.FederatedSearchPalette.overBudget',
                      'Buffer too large to index — results unavailable for this pane.'
                    )}
                  </div>
                ) : (
                  group.matches.map((match) => (
                    <CommandItem
                      key={`${group.key}:${match.absRow}:${match.col}`}
                      value={`${group.key}:${match.absRow}:${match.col}`}
                      onSelect={() => handleSelect(group, match)}
                      className="flex items-center gap-2 px-3 py-1.5"
                    >
                      {match.snippet !== null ? (
                        <span className="truncate font-mono text-xs text-foreground">
                          {match.snippet}
                        </span>
                      ) : (
                        <span className="truncate text-xs text-muted-foreground">
                          {translate(
                            'auto.components.FederatedSearchPalette.rowOnly',
                            'Row {{value0}}',
                            {
                              value0: match.absRow
                            }
                          )}
                        </span>
                      )}
                      <span className="ml-auto flex-shrink-0 text-[10px] text-muted-foreground">
                        {match.absRow}:{match.col}
                      </span>
                    </CommandItem>
                  ))
                )}
                {group.total > group.matches.length ? (
                  <div className="px-3 py-1 text-[11px] text-muted-foreground">
                    {translate(
                      'auto.components.FederatedSearchPalette.moreMatches',
                      '+{{value0}} more',
                      { value0: group.total - group.matches.length }
                    )}
                  </div>
                ) : null}
              </CommandGroup>
            )
          })
        )}
        {snapshot.pending && snapshot.query !== '' ? (
          <div className="px-3 py-2 text-[11px] text-muted-foreground">
            {translate('auto.components.FederatedSearchPalette.searching', 'Searching…')}
          </div>
        ) : null}
      </CommandList>
      {/* Accessibility: announce result count changes */}
      <div aria-live="polite" className="sr-only">
        {snapshot.query
          ? translate(
              'auto.components.FederatedSearchPalette.resultsAnnouncement',
              '{{value0}} matches shown',
              { value0: totalShown }
            )
          : ''}
      </div>
    </CommandDialog>
  )
}
