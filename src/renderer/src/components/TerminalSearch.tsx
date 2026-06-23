import { useEffect, useState, useCallback } from 'react'
import { ChevronUp, ChevronDown, X, CaseSensitive, Regex } from 'lucide-react'
import type { SearchAddon } from '@xterm/addon-search'
import { Button } from '@/components/ui/button'
import type { SearchState } from '@/components/terminal-pane/keyboard-handlers'
import { translate } from '@/i18n/i18n'
import { getFindRequestQuery } from '@/lib/find-query-bounds'

/** The aterm in-page renderer's search surface (subset of AtermPaneController).
 *  When the active pane is aterm-rendered, find/next/prev/clear route here
 *  instead of through the (absent) xterm SearchAddon. */
export type AtermSearchSurface = {
  findMatches: (query: string, caseSensitive: boolean) => number
  findNextMatch: () => void
  findPreviousMatch: () => void
  clearSearch: () => void
  searchMatchCount: () => number
  searchActiveMatchIndex: () => number
}

type TerminalSearchProps = {
  isOpen: boolean
  onClose: () => void
  searchAddon: SearchAddon | null
  /** Present when the active pane uses the aterm renderer; routes search to the
   *  canvas controller (the xterm SearchAddon is null for these panes). */
  atermSearch?: AtermSearchSurface | null
  searchStateRef: React.RefObject<SearchState>
}

export default function TerminalSearch({
  isOpen,
  onClose,
  searchAddon,
  atermSearch,
  searchStateRef
}: TerminalSearchProps): React.JSX.Element | null {
  const [query, setQuery] = useState('')
  const [caseSensitive, setCaseSensitive] = useState(false)
  const [regex, setRegex] = useState(false)
  // Match-count label ("3 / 12"), driven by the aterm controller's exact counts.
  const [matchLabel, setMatchLabel] = useState('')
  const requestQuery = getFindRequestQuery(query)

  // Why: the default xterm SearchAddon highlights blend into common
  // terminal backgrounds (see orca#612). Providing explicit decoration
  // colors gives all matches a visible yellow background and the
  // current match a brighter orange, matching the contrast VS Code and
  // iTerm2 use for terminal search. xterm requires #RRGGBB format for
  // the background colors.
  const searchOptions = useCallback(
    (incremental: boolean = false) => ({
      caseSensitive,
      regex,
      incremental,
      decorations: {
        matchBackground: '#5c4a00',
        matchBorder: '#5c4a00',
        matchOverviewRuler: '#ffcc00',
        activeMatchBackground: '#c4580e',
        activeMatchBorder: '#ffcf6b',
        activeMatchColorOverviewRuler: '#ff9900'
      }
    }),
    [caseSensitive, regex]
  )

  // Reflect the aterm controller's exact match count ("active / total") in the
  // label. The xterm SearchAddon surfaces counts via a different (async) callback
  // not wired here, so the label is aterm-only for now.
  const syncAtermMatchLabel = useCallback(() => {
    if (!atermSearch) {
      return
    }
    const total = atermSearch.searchMatchCount()
    setMatchLabel(total === 0 ? '0' : `${atermSearch.searchActiveMatchIndex()} / ${total}`)
  }, [atermSearch])

  const findNext = useCallback(() => {
    if (atermSearch) {
      atermSearch.findNextMatch()
      syncAtermMatchLabel()
      return
    }
    if (searchAddon && requestQuery) {
      searchAddon.findNext(requestQuery, searchOptions())
    }
  }, [atermSearch, searchAddon, requestQuery, searchOptions, syncAtermMatchLabel])

  const findPrevious = useCallback(() => {
    if (atermSearch) {
      atermSearch.findPreviousMatch()
      syncAtermMatchLabel()
      return
    }
    if (searchAddon && requestQuery) {
      searchAddon.findPrevious(requestQuery, searchOptions())
    }
  }, [atermSearch, searchAddon, requestQuery, searchOptions, syncAtermMatchLabel])

  const handleInputRef = useCallback((input: HTMLInputElement | null): void => {
    input?.focus()
  }, [])

  useEffect(() => {
    // Keep the ref in sync so the keyboard handler (Cmd+G / Cmd+Shift+G)
    // can read the current search state without lifting it to parent state.
    searchStateRef.current = { query: requestQuery ?? '', caseSensitive, regex }

    if (!isOpen) {
      searchAddon?.clearDecorations()
      atermSearch?.clearSearch()
      setMatchLabel('')
      return
    }
    if (!requestQuery) {
      searchAddon?.clearDecorations()
      atermSearch?.clearSearch()
      setMatchLabel('')
      return
    }
    // aterm panes: run the canvas search (highlight + scroll-to-match). aterm's
    // engine search is plain substring (regex not exposed), so `regex` is ignored
    // there for now; case sensitivity is honored.
    if (atermSearch) {
      const total = atermSearch.findMatches(requestQuery, caseSensitive)
      setMatchLabel(total === 0 ? '0' : `${atermSearch.searchActiveMatchIndex()} / ${total}`)
      return
    }
    if (searchAddon) {
      searchAddon.findNext(requestQuery, searchOptions(true))
    }
  }, [
    requestQuery,
    searchAddon,
    atermSearch,
    isOpen,
    caseSensitive,
    regex,
    searchStateRef,
    searchOptions
  ])

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      e.stopPropagation()

      if (e.key === 'Escape') {
        onClose()
      } else if (e.key === 'Enter' && e.shiftKey) {
        findPrevious()
      } else if (e.key === 'Enter') {
        findNext()
      }
    },
    [onClose, findNext, findPrevious]
  )

  if (!isOpen) {
    return null
  }

  return (
    <div
      data-terminal-search-root
      className="absolute top-2 right-2 z-50 flex items-center gap-1 rounded-lg border border-zinc-700 bg-zinc-800/95 px-2 py-1 shadow-lg backdrop-blur-sm"
      style={{ width: 300 }}
      onKeyDown={handleKeyDown}
    >
      <input
        ref={handleInputRef}
        type="text"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        placeholder={translate('auto.components.TerminalSearch.e07012f26e', 'Search...')}
        className="min-w-0 flex-1 border-none bg-transparent text-sm text-white outline-none placeholder:text-zinc-500"
      />

      {matchLabel && (
        <span className="shrink-0 px-1 text-xs tabular-nums text-zinc-400" data-terminal-search-count>
          {matchLabel}
        </span>
      )}

      <Button
        type="button"
        variant="ghost"
        size="icon-xs"
        onClick={() => setCaseSensitive((v) => !v)}
        className={`flex size-6 shrink-0 items-center justify-center rounded ${
          caseSensitive ? 'bg-zinc-700/50 text-blue-400' : 'text-zinc-400 hover:text-zinc-200'
        }`}
        title={translate('auto.components.TerminalSearch.90c61387d9', 'Case sensitive')}
      >
        <CaseSensitive size={14} />
      </Button>

      <Button
        type="button"
        variant="ghost"
        size="icon-xs"
        onClick={() => setRegex((v) => !v)}
        className={`flex size-6 shrink-0 items-center justify-center rounded ${
          regex ? 'bg-zinc-700/50 text-blue-400' : 'text-zinc-400 hover:text-zinc-200'
        }`}
        title={translate('auto.components.TerminalSearch.42e466b9f1', 'Regex')}
      >
        <Regex size={14} />
      </Button>

      <div className="mx-0.5 h-4 w-px bg-zinc-700" />

      <Button
        type="button"
        variant="ghost"
        size="icon-xs"
        onClick={findPrevious}
        className="flex size-6 shrink-0 items-center justify-center rounded text-zinc-400 hover:text-zinc-200"
        title={translate('auto.components.TerminalSearch.0f3066256e', 'Previous match')}
      >
        <ChevronUp size={14} />
      </Button>

      <Button
        type="button"
        variant="ghost"
        size="icon-xs"
        onClick={findNext}
        className="flex size-6 shrink-0 items-center justify-center rounded text-zinc-400 hover:text-zinc-200"
        title={translate('auto.components.TerminalSearch.7cb40c04eb', 'Next match')}
      >
        <ChevronDown size={14} />
      </Button>

      <div className="mx-0.5 h-4 w-px bg-zinc-700" />

      <Button
        type="button"
        variant="ghost"
        size="icon-xs"
        onClick={onClose}
        className="flex size-6 shrink-0 items-center justify-center rounded text-zinc-400 hover:text-zinc-200"
        title={translate('auto.components.TerminalSearch.db234b7519', 'Close')}
      >
        <X size={14} />
      </Button>
    </div>
  )
}
