import { describe, expect, it } from 'vitest'
import { buildProjectGridTemplate, flattenProjectGroups } from './project-view-list-rows'
import { ACTION_COLUMN_WIDTH, MIN_COLUMN_WIDTH } from './column-widths'
import type { ProjectGroup } from '../../../../shared/github-project-group-sort'
import type { GitHubProjectField, GitHubProjectRow } from '../../../../shared/github-project-types'

function makeRow(id: string): GitHubProjectRow {
  return {
    id,
    itemType: 'ISSUE',
    content: {
      number: 1,
      title: id,
      body: null,
      url: null,
      state: 'open',
      stateReason: null,
      isDraft: null,
      repository: 'acme/repo',
      assignees: [],
      labels: [],
      parentIssue: null,
      issueType: null
    },
    fieldValuesByFieldId: {},
    updatedAt: '2026-01-01T00:00:00Z',
    position: 0
  }
}

function makeGroup(key: string, rowIds: string[]): ProjectGroup {
  return { key, label: key, iteration: null, rows: rowIds.map(makeRow) }
}

function field(id: string, dataType: string): GitHubProjectField {
  return { kind: 'field', id, name: id, dataType }
}

describe('flattenProjectGroups', () => {
  it('emits no header items when the view is not grouped', () => {
    const groups = [makeGroup('all', ['r1', 'r2', 'r3'])]
    const items = flattenProjectGroups(groups, new Set(), false)
    expect(items.map((i) => i.kind)).toEqual(['row', 'row', 'row'])
    expect(items.every((i) => i.kind === 'row' && i.groupKey === 'all')).toBe(true)
  })

  it('interleaves a header before each grouped bucket in order', () => {
    const groups = [makeGroup('todo', ['a', 'b']), makeGroup('done', ['c'])]
    const items = flattenProjectGroups(groups, new Set(), true)
    expect(
      items.map((i) => (i.kind === 'group-header' ? `H:${i.group.key}` : `R:${i.row.id}`))
    ).toEqual(['H:todo', 'R:a', 'R:b', 'H:done', 'R:c'])
  })

  it('renders only the header for a collapsed group but keeps its neighbors expanded', () => {
    const groups = [makeGroup('todo', ['a', 'b']), makeGroup('done', ['c'])]
    const items = flattenProjectGroups(groups, new Set(['todo']), true)
    expect(
      items.map((i) => (i.kind === 'group-header' ? `H:${i.group.key}` : `R:${i.row.id}`))
    ).toEqual(['H:todo', 'H:done', 'R:c'])
  })
})

describe('buildProjectGridTemplate', () => {
  it('pins the first two columns to fixed pixels and flexes the rest, plus a fixed action column', () => {
    const fields = [
      field('title', 'TITLE'),
      field('status', 'SINGLE_SELECT'),
      field('assignees', 'ASSIGNEES')
    ]
    const template = buildProjectGridTemplate(fields, {})
    const parts = template.split(' ')
    // Two frozen px columns, one flexing minmax column, one fixed action column.
    expect(parts[0]).toMatch(/px$/)
    expect(parts[1]).toMatch(/px$/)
    expect(template).toContain(`minmax(${MIN_COLUMN_WIDTH}px,`)
    expect(template.endsWith(`${ACTION_COLUMN_WIDTH}px`)).toBe(true)
  })

  it('honors a stored width override for a column', () => {
    const fields = [field('title', 'TITLE'), field('status', 'SINGLE_SELECT')]
    const template = buildProjectGridTemplate(fields, { title: 500 })
    expect(template.startsWith('500px ')).toBe(true)
  })
})
