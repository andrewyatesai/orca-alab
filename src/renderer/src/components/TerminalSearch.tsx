import { useEffect, useRef, useState, useCallback, type ReactElement, type ReactNode } from 'react'
import { ChevronUp, ChevronDown, X, CaseSensitive, Regex } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import type { SearchState } from '@/components/terminal-pane/keyboard-handlers'
import { translate } from '@/i18n/i18n'
import { getFindRequestQuery } from '@/lib/find-query-bounds'

/** Find-as-you-type debounce: a full-scrollback search per KEYSTROKE stalls the engine
 *  at deep scrollback; one search per settled ~75ms window keeps typing smooth while
 *  still feeling immediate. Enter bypasses it (flushes the armed find now). */
export const SEARCH_DEBOUNCE_MS = 75

/** The aterm in-page renderer's search surface (subset of AtermPaneController).
 *  find/next/prev/clear route through the canvas controller. */
export type AtermSearchSurface = {
  findMatches: (query: string, caseSensitive: boolean, isRegex: boolean) => number
  /** Awaitable find (drives the pending indicator): resolves the post-find
   *  `{count, activeIndex}`, or null when a newer find superseded this request —
   *  the stale result must be discarded, never shown. */
  findMatchesAsync: (
    query: string,
    caseSensitive: boolean,
    isRegex: boolean
  ) => Promise<{ count: number; activeIndex: number } | null>
  findNextMatch: () => void
  findPreviousMatch: () => void
  clearSearch: () => void
  searchMatchCount: () => number
  searchActiveMatchIndex: () => number
  /** True while streaming's cost gate serves results older than the buffer content
   *  (worker path) — rendered as the ~approximate-count indicator. */
  searchResultsStale: () => boolean
  /** True when the engine reported a truncated match index (eviction / match cap,
   *  E9a) — the count is a floor, rendered "N+" (with stale it composes to "~N+"). */
  searchResultsIncomplete: () => boolean
  /** True while an issued find's results haven't landed (worker path): the count is
   *  still the previous query's, so the label shows "~N, searching…" instead. */
  searchIsPending: () => boolean
  /** Subscribe to async search-state updates; returns a disposer. On the default off-main
   *  worker path the count/active-index land a frame after find/next/prev, so the label
   *  must re-read when they arrive (no-op disposer in-process). */
  onSearchStateChange: (handler: () => void) => () => void
}

type TerminalSearchProps = {
  isOpen: boolean
  onClose: () => void
  /** The active pane's aterm search surface (the canvas controller). */
  atermSearch?: AtermSearchSurface | null
  searchStateRef: React.RefObject<SearchState>
  /** One-shot query seed consumed on open (context menu "Search for …", CM-A1):
   *  the seed becomes the query and runs a debounce-bypassed find, then the ref
   *  is nulled — typing afterwards behaves exactly as today. */
  seedQueryRef?: React.RefObject<string | null>
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
  searchStateRef,
  seedQueryRef
}: TerminalSearchProps): React.JSX.Element | null {
  const [query, setQuery] = useState('')
  const [caseSensitive, setCaseSensitive] = useState(false)
  const [regexEnabled, setRegexEnabled] = useState(false)
  // The aterm engine compiles the pattern via search_results_opts(is_regex)
  // (invalid pattern → 0 matches).
  const regex = regexEnabled
  // Match-count label ("3 / 12"), driven by the aterm controller's exact counts.
  const [matchLabel, setMatchLabel] = useState('')
  // Find in flight (debounce settled, result not landed) — rendered as the pending "…".
  const [pending, setPending] = useState(false)
  // Streaming cost gate serving results older than the content — "~" approximate label.
  const [resultsStale, setResultsStale] = useState(false)
  // Engine index truncated (eviction / match cap) — the count is a floor, "+" suffix.
  const [resultsIncomplete, setResultsIncomplete] = useState(false)
  // Monotonic find generation: only the NEWEST request may clear pending / set the
  // label, so a slow superseded find can never overwrite fresher results.
  const findSeqRef = useRef(0)
  const debounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  // The armed-but-not-yet-run find (for the Enter debounce bypass).
  const armedFindRef = useRef<{ query: string; caseSensitive: boolean; regex: boolean } | null>(
    null
  )
  // Set when the query change came from the one-shot open seed: the next query
  // effect runs the find immediately (same bypass as Enter) instead of debouncing.
  const seedBypassRef = useRef(false)
  const requestQuery = getFindRequestQuery(query)

  // Consume the one-shot seed on open (context menu "Search for …").
  useEffect(() => {
    if (!isOpen) {
      return
    }
    const seed = seedQueryRef?.current
    if (!seed) {
      return
    }
    seedQueryRef.current = null
    seedBypassRef.current = true
    setQuery(seed)
  }, [isOpen, seedQueryRef])

  // Reflect the aterm controller's exact match count ("active / total") + stale flag,
  // or an honest approximation while an issued find is still in flight on the worker
  // path (the snapshot count is the PREVIOUS query's until the worker echoes back).
  const syncAtermMatchLabel = useCallback(() => {
    if (!atermSearch) {
      return
    }
    const total = atermSearch.searchMatchCount()
    if (atermSearch.searchIsPending()) {
      const searching = translate('auto.components.TerminalSearch.searchingPending', 'searching…')
      setMatchLabel(total === 0 ? searching : `~${total}, ${searching}`)
      setResultsStale(false)
      setResultsIncomplete(false)
      return
    }
    setMatchLabel(total === 0 ? '0' : `${atermSearch.searchActiveMatchIndex()} / ${total}`)
    setResultsStale(atermSearch.searchResultsStale())
    setResultsIncomplete(atermSearch.searchResultsIncomplete())
  }, [atermSearch])

  // Issue the find NOW; pending shows until THIS request's result lands. A null
  // resolution means a newer find superseded it — that newer one owns the label.
  const runFind = useCallback(
    (findQuery: string, findCaseSensitive: boolean, findRegex: boolean) => {
      if (!atermSearch) {
        return
      }
      const seq = ++findSeqRef.current
      setPending(true)
      void atermSearch.findMatchesAsync(findQuery, findCaseSensitive, findRegex).then((result) => {
        if (seq !== findSeqRef.current) {
          return
        }
        setPending(false)
        if (result) {
          setMatchLabel(result.count === 0 ? '0' : `${result.activeIndex} / ${result.count}`)
          setResultsStale(atermSearch.searchResultsStale())
          setResultsIncomplete(atermSearch.searchResultsIncomplete())
        } else {
          // Timed-out/disposed round-trip: fall back to the snapshot-backed label
          // (onSearchStateChange re-syncs it when the worker's state lands).
          syncAtermMatchLabel()
        }
      })
    },
    [atermSearch, syncAtermMatchLabel]
  )

  // Enter bypass: run the debounced find IMMEDIATELY instead of waiting out the timer.
  // Returns true when a find was armed (Enter searched now instead of navigating).
  const flushArmedFind = useCallback((): boolean => {
    if (debounceTimerRef.current === null) {
      return false
    }
    clearTimeout(debounceTimerRef.current)
    debounceTimerRef.current = null
    const armed = armedFindRef.current
    armedFindRef.current = null
    if (!armed) {
      return false
    }
    runFind(armed.query, armed.caseSensitive, armed.regex)
    return true
  }, [runFind])

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
    // Deliberately OUTSIDE the debounce: the handler must see each keystroke.
    searchStateRef.current = { query: requestQuery ?? '', caseSensitive, regex }

    if (!isOpen || !requestQuery) {
      // Clearing is immediate (never debounced) and invalidates any in-flight find.
      armedFindRef.current = null
      findSeqRef.current++
      setPending(false)
      setResultsStale(false)
      setResultsIncomplete(false)
      atermSearch?.clearSearch()
      setMatchLabel('')
      return
    }
    // Seeded query (menu "Search for …"): run NOW — the user already committed to
    // this exact query, so the keystroke debounce would only add latency.
    if (seedBypassRef.current) {
      seedBypassRef.current = false
      runFind(requestQuery, caseSensitive, regex)
      return
    }
    // Debounced find-as-you-type: one engine search per settled window, not one per
    // keystroke. The armed args let Enter flush the very same find immediately.
    armedFindRef.current = { query: requestQuery, caseSensitive, regex }
    debounceTimerRef.current = setTimeout(() => {
      debounceTimerRef.current = null
      armedFindRef.current = null
      runFind(requestQuery, caseSensitive, regex)
    }, SEARCH_DEBOUNCE_MS)
    // Label the debounce window from the snapshot now — while an earlier find is
    // still in flight it reads "~N, searching…" instead of claiming a stale count.
    syncAtermMatchLabel()
    return () => {
      if (debounceTimerRef.current !== null) {
        clearTimeout(debounceTimerRef.current)
        debounceTimerRef.current = null
      }
    }
  }, [
    requestQuery,
    atermSearch,
    isOpen,
    caseSensitive,
    regex,
    searchStateRef,
    runFind,
    syncAtermMatchLabel
  ])

  // Worker path: search count/active-index arrive a frame after find/next/prev, so re-sync
  // the label when the worker pushes them. No-op disposer in-process (count is synchronous).
  useEffect(() => {
    if (!atermSearch || !isOpen) {
      return
    }
    return atermSearch.onSearchStateChange(syncAtermMatchLabel)
  }, [atermSearch, isOpen, syncAtermMatchLabel])

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      e.stopPropagation()

      if (e.key === 'Escape') {
        onClose()
      } else if (e.key === 'Enter') {
        // Enter bypasses the debounce: a still-armed find runs NOW; otherwise it
        // navigates. (A posted nav lands after any in-flight find — worker FIFO —
        // so it always steps the fresh result set.)
        if (flushArmedFind()) {
          return
        }
        if (e.shiftKey) {
          findPrevious()
        } else {
          findNext()
        }
      }
    },
    [onClose, findNext, findPrevious, flushArmedFind]
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

      {pending ? (
        // Visible pending state: the debounce settled but the engine result hasn't
        // landed yet (worker round-trip / large scrollback).
        <span
          className="shrink-0 px-1 text-xs tabular-nums text-muted-foreground"
          data-terminal-search-pending
        >
          …
        </span>
      ) : (
        matchLabel && (
          <span
            className="shrink-0 px-1 text-xs tabular-nums text-muted-foreground"
            data-terminal-search-count
            data-stale={resultsStale || undefined}
            data-incomplete={resultsIncomplete || undefined}
          >
            {/* "~" = approximate (streaming's cost gate serves results older than the
                buffer; the trailing re-index removes it). "+" = incomplete index (the
                engine truncated matches — the count is a floor). Both compose: "~N+". */}
            {`${resultsStale ? '~' : ''}${matchLabel}${resultsIncomplete ? '+' : ''}`}
          </span>
        )
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
