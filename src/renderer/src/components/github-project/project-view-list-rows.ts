// Why: the flattened row model + grid-template builder for the virtualized
// project table. Kept beside ProjectViewList so the component stays under the
// file-length budget while the list-shaping logic remains independently testable.
import { ACTION_COLUMN_WIDTH, MIN_COLUMN_WIDTH, resolveWidth } from './column-widths'
import type { ProjectGroup } from '../../../../shared/github-project-group-sort'
import type {
  GitHubProjectField,
  GitHubProjectRow,
  GitHubProjectSortDirection
} from '../../../../shared/github-project-types'

// Why: a column-header click applies a transient, unpersisted sort override
// shared between the list and its header row.
export type SortOverride = { fieldId: string; direction: GitHubProjectSortDirection }

// Why: virtualization only mounts on-screen rows, so estimates just seed the
// scroll size before measureElement reports each row's real height. Overscan
// keeps a few rows above/below the viewport mounted so fast scrolls stay filled.
export const PROJECT_GROUP_HEADER_ESTIMATED_HEIGHT = 33
export const PROJECT_ROW_ESTIMATED_HEIGHT = 40
export const PROJECT_ROW_OVERSCAN = 8

// Flattened, virtualization-friendly view of the grouped rows: group headers
// interleave with the rows they contain. Collapsed groups contribute only a
// header. When the view has no group-by field there are no header items.
export type ProjectListItem =
  | { kind: 'group-header'; group: ProjectGroup }
  | { kind: 'row'; row: GitHubProjectRow; groupKey: string }

export function buildProjectGridTemplate(
  fields: GitHubProjectField[],
  widths: Readonly<Record<string, number>>
): string {
  // Why: the first two columns are frozen during horizontal scroll, so their
  // actual widths must be deterministic for the second sticky offset.
  const cols = fields.map((field, index) =>
    index < 2
      ? `${resolveWidth(field, widths)}px`
      : `minmax(${MIN_COLUMN_WIDTH}px, ${resolveWidth(field, widths)}fr)`
  )
  cols.push(`${ACTION_COLUMN_WIDTH}px`)
  return cols.join(' ')
}

// Flatten groups + rows into one list the virtualizer can index. Collapsed
// groups contribute only their header, preserving the collapse behavior; when
// the view has no group-by field, no header rows are emitted.
export function flattenProjectGroups(
  groups: readonly ProjectGroup[],
  collapsed: ReadonlySet<string>,
  hasGroupBy: boolean
): ProjectListItem[] {
  const out: ProjectListItem[] = []
  for (const group of groups) {
    if (hasGroupBy) {
      out.push({ kind: 'group-header', group })
    }
    if (!collapsed.has(group.key)) {
      for (const row of group.rows) {
        out.push({ kind: 'row', row, groupKey: group.key })
      }
    }
  }
  return out
}
