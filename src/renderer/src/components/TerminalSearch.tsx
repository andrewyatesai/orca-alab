import { useEffect, useState, useCallback, type ReactElement, type ReactNode } from 'react'
import { ChevronUp, ChevronDown, X, CaseSensitive, Regex } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import type { SearchState } from '@/components/terminal-pane/keyboard-handlers'
import { translate } from '@/i18n/i18n'
import { getFindRequestQuery } from '@/lib/find-query-bounds'

/** The aterm in-page renderer's search surface (subset of AtermPaneController).
 *  find/next/prev/clear route through the canvas controller. */
export type AtermSearchSurface = {
  findMatches: (query: string, caseSensitive: boolean, isRegex: boolean) => number
  findNextMatch: () => void
  findPreviousMatch: () => void
  clearSearch: () => void
  searchMatchCount: () => number
  searchActiveMatchIndex: () => number
}

type TerminalSearchProps = {
  isOpen: boolean
  onClose: () => void
  /** The active pane's aterm search surface (the canvas controller). */
  atermSearch?: AtermSearchSurface | null
  searchStateRef: React.RefObject<SearchState>
}

// A ghost icon-button with a styleguide Tooltip (replaces native title= attrs).
function SearchButton({
  tip,
  className,
  disabled,
  onClick,
  children
}: {
  tip: string
  className: string
  disabled?: boolean
  onClick: () => void
  children: ReactNode
}): ReactElement {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          disabled={disabled}
          onClick={onClick}
          className={className}
        >
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{tip}</TooltipContent>
    </Tooltip>
  )
}

export default function TerminalSearch({
  isOpen,
  onClose,
  atermSearch,
  searchStateRef
}: TerminalSearchProps): React.JSX.Element | null {
  const [query, setQuery] = useState('')
  const [caseSensitive, setCaseSensitive] = useState(false)
  const [regexEnabled, setRegexEnabled] = useState(false)
  // The aterm engine compiles the pattern via search_results_opts(is_regex)
  // (invalid pattern → 0 matches).
  const regex = regexEnabled
  // Match-count label ("3 / 12"), driven by the aterm controller's exact counts.
  const [matchLabel, setMatchLabel] = useState('')
  const requestQuery = getFindRequestQuery(query)

  // Reflect the aterm controller's exact match count ("active / total").
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
    }
  }, [atermSearch, syncAtermMatchLabel])

  const findPrevious = useCallback(() => {
    if (atermSearch) {
      atermSearch.findPreviousMatch()
      syncAtermMatchLabel()
    }
  }, [atermSearch, syncAtermMatchLabel])

  const handleInputRef = useCallback((input: HTMLInputElement | null): void => {
    input?.focus()
  }, [])

  useEffect(() => {
    // Keep the ref in sync so the keyboard handler (Cmd+G / Cmd+Shift+G)
    // can read the current search state without lifting it to parent state.
    searchStateRef.current = { query: requestQuery ?? '', caseSensitive, regex }

    if (!isOpen || !requestQuery) {
      atermSearch?.clearSearch()
      setMatchLabel('')
      return
    }
    // Run the canvas search (highlight + scroll-to-match) honoring both case
    // sensitivity and the regex toggle (the engine compiles the pattern).
    if (atermSearch) {
      const total = atermSearch.findMatches(requestQuery, caseSensitive, regex)
      setMatchLabel(total === 0 ? '0' : `${atermSearch.searchActiveMatchIndex()} / ${total}`)
    }
  }, [requestQuery, atermSearch, isOpen, caseSensitive, regex, searchStateRef])

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
      className="absolute top-2 right-2 z-50 flex items-center gap-1 rounded-lg border border-border bg-popover/95 px-2 py-1 shadow-md backdrop-blur-sm"
      style={{ width: 300 }}
      onKeyDown={handleKeyDown}
    >
      <input
        ref={handleInputRef}
        type="text"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        placeholder={translate('auto.components.TerminalSearch.e07012f26e', 'Search...')}
        className="min-w-0 flex-1 border-none bg-transparent text-sm text-popover-foreground outline-none placeholder:text-muted-foreground"
      />

      {matchLabel && (
        <span
          className="shrink-0 px-1 text-xs tabular-nums text-muted-foreground"
          data-terminal-search-count
        >
          {matchLabel}
        </span>
      )}

      <SearchButton
        tip={translate('auto.components.TerminalSearch.90c61387d9', 'Case sensitive')}
        onClick={() => setCaseSensitive((v) => !v)}
        className={`flex size-6 shrink-0 items-center justify-center rounded ${
          caseSensitive
            ? 'bg-accent text-accent-foreground'
            : 'text-muted-foreground hover:text-foreground'
        }`}
      >
        <CaseSensitive size={14} />
      </SearchButton>

      <SearchButton
        tip={translate('auto.components.TerminalSearch.42e466b9f1', 'Regex')}
        onClick={() => setRegexEnabled((v) => !v)}
        className={`flex size-6 shrink-0 items-center justify-center rounded ${
          regex ? 'bg-accent text-accent-foreground' : 'text-muted-foreground hover:text-foreground'
        }`}
      >
        <Regex size={14} />
      </SearchButton>

      <div className="mx-0.5 h-4 w-px bg-border" />

      <SearchButton
        tip={translate('auto.components.TerminalSearch.0f3066256e', 'Previous match')}
        onClick={findPrevious}
        className="flex size-6 shrink-0 items-center justify-center rounded text-muted-foreground hover:text-foreground"
      >
        <ChevronUp size={14} />
      </SearchButton>

      <SearchButton
        tip={translate('auto.components.TerminalSearch.7cb40c04eb', 'Next match')}
        onClick={findNext}
        className="flex size-6 shrink-0 items-center justify-center rounded text-muted-foreground hover:text-foreground"
      >
        <ChevronDown size={14} />
      </SearchButton>

      <div className="mx-0.5 h-4 w-px bg-border" />

      <SearchButton
        tip={translate('auto.components.TerminalSearch.db234b7519', 'Close')}
        onClick={onClose}
        className="flex size-6 shrink-0 items-center justify-center rounded text-muted-foreground hover:text-foreground"
      >
        <X size={14} />
      </SearchButton>
    </div>
  )
}
