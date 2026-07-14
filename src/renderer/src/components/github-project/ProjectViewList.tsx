import React, { useCallback, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { useVirtualizer } from '@tanstack/react-virtual'
import ProjectGroupHeader from './ProjectGroupHeader'
import ProjectHeaderRow from './ProjectHeaderRow'
import ProjectRow from './ProjectRow'
import { groupRows, sortRows } from '../../../../shared/github-project-group-sort'
import { getAvailableColumns, loadHiddenColumns, saveHiddenColumns } from './columns'
import { loadColumnWidths, MIN_COLUMN_WIDTH, saveColumnWidths } from './column-widths'
import {
  buildProjectGridTemplate,
  flattenProjectGroups,
  PROJECT_GROUP_HEADER_ESTIMATED_HEIGHT,
  PROJECT_ROW_ESTIMATED_HEIGHT,
  PROJECT_ROW_OVERSCAN,
  type ProjectListItem,
  type SortOverride
} from './project-view-list-rows'
import type {
  GitHubIssueType,
  GitHubProjectFieldMutationValue,
  GitHubProjectRow,
  GitHubProjectTable
} from '../../../../shared/github-project-types'
import type { GlobalSettings } from '../../../../shared/types'
import { translate } from '@/i18n/i18n'

type Props = {
  table: GitHubProjectTable
  onOpenDialog?: (row: GitHubProjectRow) => void
  onEditField?: (
    row: GitHubProjectRow,
    fieldId: string,
    value: GitHubProjectFieldMutationValue | null
  ) => void
  onEditAssignees?: (row: GitHubProjectRow, add: string[], remove: string[]) => void
  onEditLabels?: (row: GitHubProjectRow, add: string[], remove: string[]) => void
  onEditIssueType?: (row: GitHubProjectRow, issueType: GitHubIssueType | null) => void
  onStartWork?: (row: GitHubProjectRow) => void
  onOpenInBrowser?: (row: GitHubProjectRow) => void
  sourceSettings: Pick<GlobalSettings, 'activeRuntimeEnvironmentId'> | null | undefined
}

export default function ProjectViewList({
  table,
  onOpenDialog,
  onEditField,
  onEditAssignees,
  onEditLabels,
  onEditIssueType,
  onStartWork,
  onOpenInBrowser,
  sourceSettings
}: Props): React.JSX.Element {
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(() => new Set())
  // Why: column-header clicks override the view's saved sortByFields locally
  // without persisting to GitHub — matches GitHub Projects' transient
  // header-sort behavior. `null` means "use the view's sort as authored".
  const [sortOverride, setSortOverride] = useState<SortOverride | null>(null)

  // Why: include project id so the same view id colliding across projects
  // doesn't cross-pollute hidden-column preferences.
  const scopeKey = `${table.project.id}:${table.selectedView.id}`
  const availableFields = useMemo(
    () => getAvailableColumns(table.selectedView),
    [table.selectedView]
  )
  // Why: switching project views should not paint one commit with the
  // previous view's local column preferences before an Effect catches up.
  const persistedHidden = useMemo(() => loadHiddenColumns(scopeKey), [scopeKey])
  const [hiddenByScope, setHiddenByScope] = useState<
    Readonly<Record<string, ReadonlySet<string> | undefined>>
  >({})
  const hidden = hiddenByScope[scopeKey] ?? persistedHidden
  const fields = useMemo(
    () => availableFields.filter((f) => !hidden.has(f.id)),
    [availableFields, hidden]
  )

  const persistedWidths = useMemo(() => loadColumnWidths(scopeKey), [scopeKey])
  const [widthsByScope, setWidthsByScope] = useState<
    Readonly<Record<string, Readonly<Record<string, number>> | undefined>>
  >({})
  const widths = widthsByScope[scopeKey] ?? persistedWidths

  const scrollRef = useRef<HTMLDivElement | null>(null)
  const headerRef = useRef<HTMLDivElement | null>(null)

  const setColumnPair = useCallback(
    (fieldId: string, width: number, nextFieldId: string, nextWidth: number): void => {
      setWidthsByScope((prev) => {
        const currentWidths = prev[scopeKey] ?? persistedWidths
        const updated = {
          ...currentWidths,
          [fieldId]: Math.max(MIN_COLUMN_WIDTH, Math.round(width)),
          [nextFieldId]: Math.max(MIN_COLUMN_WIDTH, Math.round(nextWidth))
        }
        saveColumnWidths(scopeKey, updated)
        return { ...prev, [scopeKey]: updated }
      })
    },
    [persistedWidths, scopeKey]
  )

  const gridTemplate = useMemo(() => buildProjectGridTemplate(fields, widths), [fields, widths])

  // Why: drive the live resize width through a CSS variable during the drag —
  // mutating the grid template repaints the header + every row via the cascade
  // with zero React renders. Only mouse-up (setColumnPair) touches state.
  const previewColumnPair = useCallback(
    (fieldId: string, width: number, nextFieldId: string, nextWidth: number): void => {
      const previewWidths = {
        ...widths,
        [fieldId]: Math.max(MIN_COLUMN_WIDTH, Math.round(width)),
        [nextFieldId]: Math.max(MIN_COLUMN_WIDTH, Math.round(nextWidth))
      }
      scrollRef.current?.style.setProperty(
        '--project-grid-template',
        buildProjectGridTemplate(fields, previewWidths)
      )
    },
    [fields, widths]
  )

  const handleListScroll = useCallback((event: React.UIEvent<HTMLDivElement>): void => {
    // Why: frozen columns need the horizontal offset, but piping every scroll
    // tick through React state rerenders the entire project row set. Set it
    // imperatively (not via an inline style prop) so virtualizer re-renders
    // during vertical scroll don't reset the horizontal offset to 0.
    event.currentTarget.style.setProperty(
      '--project-scroll-left',
      `${event.currentTarget.scrollLeft}px`
    )
  }, [])

  const toggleGroup = useCallback((key: string): void => {
    setCollapsed((prev) => {
      const next = new Set(prev)
      if (next.has(key)) {
        next.delete(key)
      } else {
        next.add(key)
      }
      return next
    })
  }, [])

  const toggleColumn = (fieldId: string): void => {
    setHiddenByScope((prev) => {
      const next = new Set(prev[scopeKey] ?? persistedHidden)
      if (next.has(fieldId)) {
        next.delete(fieldId)
      } else {
        next.add(fieldId)
      }
      saveHiddenColumns(scopeKey, next)
      return { ...prev, [scopeKey]: next }
    })
  }

  const effectiveTable = useMemo<GitHubProjectTable>(() => {
    if (!sortOverride) {
      return table
    }
    const field = fields.find((f) => f.id === sortOverride.fieldId)
    if (!field) {
      return table
    }
    return {
      ...table,
      selectedView: {
        ...table.selectedView,
        sortByFields: [{ field, direction: sortOverride.direction }]
      }
    }
  }, [table, fields, sortOverride])

  const groups = useMemo(() => {
    // Why: sort first, then group. Sorting the flat stream ensures rows within
    // each group honor the view's sortByFields too — groupRows preserves input
    // order within each bucket.
    const sorted = sortRows(effectiveTable, effectiveTable.rows)
    return groupRows(effectiveTable, sorted)
  }, [effectiveTable])

  // Why: only render a group-header row when the view actually groups. Without
  // a group-by field, groupRows returns a single synthetic 'all' bucket that
  // must not surface a header — matching the pre-virtualization layout.
  const hasGroupBy = Boolean(table.selectedView.groupByFields[0])

  const listItems = useMemo<ProjectListItem[]>(
    () => flattenProjectGroups(groups, collapsed, hasGroupBy),
    [groups, collapsed, hasGroupBy]
  )

  // Why: the sticky column header sits above the virtual list in normal flow,
  // so the virtualizer must offset its range math (scrollMargin) by the
  // header's height. Measure it rather than hardcode, since header height can
  // shift with theme/zoom; a ResizeObserver keeps it current.
  const [headerHeight, setHeaderHeight] = useState(0)
  useLayoutEffect(() => {
    const el = headerRef.current
    if (!el) {
      return
    }
    const measure = (): void => setHeaderHeight(el.offsetHeight)
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(el)
    return () => observer.disconnect()
  }, [])

  const rowVirtualizer = useVirtualizer({
    count: listItems.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: (index) =>
      listItems[index]?.kind === 'group-header'
        ? PROJECT_GROUP_HEADER_ESTIMATED_HEIGHT
        : PROJECT_ROW_ESTIMATED_HEIGHT,
    overscan: PROJECT_ROW_OVERSCAN,
    scrollMargin: headerHeight,
    getItemKey: (index) => {
      const item = listItems[index]
      if (!item) {
        return `missing:${index}`
      }
      return item.kind === 'group-header' ? `group:${item.group.key}` : `row:${item.row.id}`
    }
  })

  const handleSortClick = (fieldId: string): void => {
    setSortOverride((prev) => {
      if (!prev || prev.fieldId !== fieldId) {
        return { fieldId, direction: 'ASC' }
      }
      if (prev.direction === 'ASC') {
        return { fieldId, direction: 'DESC' }
      }
      return null
    })
  }

  if (table.rows.length === 0) {
    return (
      <div className="flex min-h-[120px] items-center justify-center p-6 text-sm text-muted-foreground">
        {translate(
          'auto.components.github.project.ProjectViewList.4f57d2e0b1',
          "No items match this view's filter."
        )}
      </div>
    )
  }

  // Why: the visible sort indicator reflects either the local override or the
  // first persisted sort from the view, so users see what's actually driving
  // row order.
  const activeSort: SortOverride | null = sortOverride
    ? sortOverride
    : effectiveTable.selectedView.sortByFields[0]
      ? {
          fieldId: effectiveTable.selectedView.sortByFields[0].field.id,
          direction: effectiveTable.selectedView.sortByFields[0].direction
        }
      : null

  return (
    <div
      ref={scrollRef}
      className="flex min-h-0 min-w-0 flex-1 flex-col overflow-auto scrollbar-sleek"
      // Why: publish the committed grid template as a CSS variable so the header
      // + every row read it via the cascade — a column resize then repaints the
      // whole table without re-rendering any row. It lives in an inline style
      // (not a layout effect) so it is present before rows first measure, and
      // React's per-property style diff leaves the imperatively-set drag preview
      // untouched (the committed value is unchanged mid-drag).
      style={{ '--project-grid-template': gridTemplate } as React.CSSProperties}
      onScroll={handleListScroll}
    >
      <ProjectHeaderRow
        headerRef={headerRef}
        fields={fields}
        availableFields={availableFields}
        hidden={hidden}
        onToggleColumn={toggleColumn}
        activeSort={activeSort}
        onSortClick={handleSortClick}
        widths={widths}
        onPreviewColumn={previewColumnPair}
        onCommitColumn={setColumnPair}
      />
      {/* Why: only on-screen rows mount. The spacer reserves the full scroll
          height; each virtual row is absolutely positioned via translateY,
          offset by the sticky header height (scrollMargin). */}
      <div className="relative w-full" style={{ height: `${rowVirtualizer.getTotalSize()}px` }}>
        {rowVirtualizer.getVirtualItems().map((virtualItem) => {
          const item = listItems[virtualItem.index]
          if (!item) {
            return null
          }
          return (
            <div
              key={virtualItem.key}
              data-index={virtualItem.index}
              ref={rowVirtualizer.measureElement}
              className="absolute left-0 top-0 w-full"
              style={{ transform: `translateY(${virtualItem.start - headerHeight}px)` }}
            >
              {item.kind === 'group-header' ? (
                <ProjectGroupHeader
                  group={item.group}
                  expanded={!collapsed.has(item.group.key)}
                  onToggle={() => toggleGroup(item.group.key)}
                />
              ) : (
                <ProjectRow
                  row={item.row}
                  fields={fields}
                  widths={widths}
                  onPreviewResize={previewColumnPair}
                  onCommitResize={setColumnPair}
                  editable
                  onOpenDialog={onOpenDialog}
                  onEditField={onEditField}
                  onEditAssignees={onEditAssignees}
                  onEditLabels={onEditLabels}
                  onEditIssueType={onEditIssueType}
                  onStartWork={onStartWork}
                  onOpenInBrowser={onOpenInBrowser}
                  sourceSettings={sourceSettings}
                />
              )}
            </div>
          )
        })}
      </div>
    </div>
  )
}
