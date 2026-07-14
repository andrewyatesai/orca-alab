import React, { useCallback } from 'react'
import { ExternalLink, Play } from 'lucide-react'
import { HoverCard, HoverCardContent, HoverCardTrigger } from '@/components/ui/hover-card'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'
import ColumnResizeHandle from './ColumnResizeHandle'
import { resolveWidth } from './column-widths'
import ProjectCell from './ProjectCell'
import type {
  GitHubIssueType,
  GitHubProjectField,
  GitHubProjectFieldMutationValue,
  GitHubProjectRow as GitHubProjectRowType
} from '../../../../shared/github-project-types'
import type { GlobalSettings } from '../../../../shared/types'
import { translate } from '@/i18n/i18n'

const PROJECT_FROZEN_COLUMN_SURFACE_CLASS =
  '[background:color-mix(in_srgb,var(--muted)_50%,var(--background))]'
const PROJECT_FROZEN_COLUMN_HOVER_SURFACE_CLASS =
  'group-hover/project-row:[background:color-mix(in_srgb,var(--accent)_60%,var(--background))]'

type Props = {
  row: GitHubProjectRowType
  fields: GitHubProjectField[]
  widths: Readonly<Record<string, number>>
  onPreviewResize: (fieldId: string, width: number, nextFieldId: string, nextWidth: number) => void
  onCommitResize: (fieldId: string, width: number, nextFieldId: string, nextWidth: number) => void
  editable: boolean
  // Why: the row-level callbacks take the row so ProjectViewList can pass one
  // stable function identity to every row (instead of allocating a fresh inline
  // arrow per row per render). Binding the row happens here, memoized, so a
  // memoized ProjectRow stays skippable when its parent re-renders on scroll.
  onOpenDialog?: (row: GitHubProjectRowType) => void
  onEditField?: (
    row: GitHubProjectRowType,
    fieldId: string,
    value: GitHubProjectFieldMutationValue | null
  ) => void
  onEditAssignees?: (row: GitHubProjectRowType, add: string[], remove: string[]) => void
  onEditLabels?: (row: GitHubProjectRowType, add: string[], remove: string[]) => void
  onEditIssueType?: (row: GitHubProjectRowType, issueType: GitHubIssueType | null) => void
  onStartWork?: (row: GitHubProjectRowType) => void
  onOpenInBrowser?: (row: GitHubProjectRowType) => void
  sourceSettings: Pick<GlobalSettings, 'activeRuntimeEnvironmentId'> | null | undefined
}

function ProjectRow({
  row,
  fields,
  widths,
  onPreviewResize,
  onCommitResize,
  editable,
  onOpenDialog,
  onEditField,
  onEditAssignees,
  onEditLabels,
  onEditIssueType,
  onStartWork,
  onOpenInBrowser,
  sourceSettings
}: Props): React.JSX.Element {
  const disabled = row.itemType === 'REDACTED'
  // Why: bind `row` into stable per-row callbacks so the heavy ProjectCell
  // subtree (each memoized) is never reconciled just because a fresh arrow was
  // handed down. Identity only changes when the parent callback or row changes.
  const handleOpenDialog = useCallback(() => onOpenDialog?.(row), [onOpenDialog, row])
  const handleEditField = useCallback(
    (fieldId: string, value: GitHubProjectFieldMutationValue | null) =>
      onEditField?.(row, fieldId, value),
    [onEditField, row]
  )
  const handleEditAssignees = useCallback(
    (add: string[], remove: string[]) => onEditAssignees?.(row, add, remove),
    [onEditAssignees, row]
  )
  const handleEditLabels = useCallback(
    (add: string[], remove: string[]) => onEditLabels?.(row, add, remove),
    [onEditLabels, row]
  )
  const handleEditIssueType = useCallback(
    (issueType: GitHubIssueType | null) => onEditIssueType?.(row, issueType),
    [onEditIssueType, row]
  )
  const handleStartWork = useCallback(() => onStartWork?.(row), [onStartWork, row])
  const handleOpenInBrowser = useCallback(() => onOpenInBrowser?.(row), [onOpenInBrowser, row])
  // Why: design doc §Row actions — draft-issue rows have no URL or number, so
  // the title is non-interactive. Surface the draft body in a hover card so
  // the user can still read context without round-tripping to GitHub.
  const draftBody =
    row.itemType === 'DRAFT_ISSUE' && row.content.body && row.content.body.trim().length > 0
      ? row.content.body
      : null
  const rowInner = (
    <div
      className={cn(
        'group group/project-row grid min-h-10 items-stretch gap-3 border-b border-border/30 px-3 hover:bg-accent/60',
        disabled && 'opacity-60'
      )}
      // Why: read the shared grid template from a CSS variable set on the
      // scroll container so a column resize repaints every row via the CSS
      // cascade — without re-rendering (or even re-reconciling) any row.
      style={{ gridTemplateColumns: 'var(--project-grid-template)' }}
    >
      {fields.map((f, idx) => {
        const next = fields[idx + 1]
        const frozen = idx < 2
        return (
          <div
            key={f.id}
            className={cn(
              'flex min-w-0 items-stretch overflow-hidden',
              !frozen && 'relative',
              frozen &&
                cn(
                  'relative z-10 before:absolute before:-left-3 before:top-0 before:bottom-0 before:w-3 before:bg-inherit',
                  PROJECT_FROZEN_COLUMN_SURFACE_CLASS,
                  PROJECT_FROZEN_COLUMN_HOVER_SURFACE_CLASS
                ),
              idx === 1 && 'border-r border-border/40'
            )}
            style={
              frozen ? { transform: 'translateX(var(--project-scroll-left, 0px))' } : undefined
            }
          >
            <div className="flex min-w-0 flex-1 items-stretch overflow-hidden">
              <ProjectCell
                row={row}
                field={f}
                editable={editable}
                onEditField={handleEditField}
                onEditAssignees={handleEditAssignees}
                onEditLabels={handleEditLabels}
                onEditIssueType={handleEditIssueType}
                onOpenDialog={f.dataType === 'TITLE' ? handleOpenDialog : undefined}
                sourceSettings={sourceSettings}
              />
            </div>
            {next ? (
              <ColumnResizeHandle
                fieldId={f.id}
                nextFieldId={next.id}
                currentWidth={resolveWidth(f, widths)}
                nextWidth={resolveWidth(next, widths)}
                onPreview={onPreviewResize}
                onCommit={onCommitResize}
              />
            ) : null}
          </div>
        )
      })}
      <div className="flex items-center justify-end gap-1 can-hover:opacity-0 transition group-hover:opacity-100">
        {row.content.url ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                onClick={handleOpenInBrowser}
                aria-label={translate(
                  'auto.components.github.project.ProjectRow.e12be8b4d4',
                  'Open in GitHub'
                )}
                className="rounded p-1 hover:bg-muted"
              >
                <ExternalLink className="size-3.5" />
              </button>
            </TooltipTrigger>
            <TooltipContent>
              {translate('auto.components.github.project.ProjectRow.e12be8b4d4', 'Open in GitHub')}
            </TooltipContent>
          </Tooltip>
        ) : null}
        {!disabled && row.itemType !== 'DRAFT_ISSUE' && row.content.number != null ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                onClick={handleStartWork}
                aria-label={translate(
                  'auto.components.github.project.ProjectRow.75b5d816e3',
                  'Start work'
                )}
                className="rounded p-1 hover:bg-muted"
              >
                <Play className="size-3.5" />
              </button>
            </TooltipTrigger>
            <TooltipContent>
              {translate('auto.components.github.project.ProjectRow.75b5d816e3', 'Start work')}
            </TooltipContent>
          </Tooltip>
        ) : null}
      </div>
    </div>
  )

  if (draftBody) {
    return (
      <HoverCard openDelay={150}>
        <HoverCardTrigger asChild>{rowInner}</HoverCardTrigger>
        <HoverCardContent
          align="start"
          sideOffset={4}
          className="max-h-80 w-96 overflow-y-auto whitespace-pre-wrap text-xs scrollbar-sleek"
        >
          {draftBody}
        </HoverCardContent>
      </HoverCard>
    )
  }
  return rowInner
}

// Why: ~500 rows each mount a heavy per-cell tree; memoizing means a parent
// re-render (e.g. every virtualizer scroll tick) skips rows whose props are
// unchanged. The parent hands stable row-keyed callbacks + a memoized fields
// list so this comparison actually holds.
export default React.memo(ProjectRow)
