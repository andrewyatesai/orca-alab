import { describe, expect, it } from 'vitest'
import { eventSchemas, featureWallTileIdSchema } from './telemetry-events'

describe('feature wall schemas', () => {
  it('accepts supported open sources and a minimal close payload', () => {
    expect(eventSchemas.feature_wall_opened.safeParse({ source: 'help_menu' }).success).toBe(true)
    expect(eventSchemas.feature_wall_opened.safeParse({ source: 'popup' }).success).toBe(true)
    expect(eventSchemas.feature_wall_opened.safeParse({ source: 'onboarding' }).success).toBe(true)
    expect(eventSchemas.feature_wall_closed.safeParse({ dwell_ms: 1200 }).success).toBe(true)
  })

  it('accepts the complete ALab lifecycle depth summary', () => {
    expect(
      eventSchemas.feature_wall_closed.safeParse({
        dwell_ms: 1200,
        source: 'help_menu',
        exit_action: 'done',
        furthest_step: 'computer-use',
        last_group_id: 'anywhere',
        visited_workflow_count: 6,
        visited_substep_count: 14,
        completed_workflow_count: 6,
        completed_substep_count: 14
      }).success
    ).toBe(true)
  })

  it('rejects stale sources, workflow ids, and out-of-range depth', () => {
    expect(eventSchemas.feature_wall_opened.safeParse({}).success).toBe(false)
    expect(eventSchemas.feature_wall_opened.safeParse({ source: 'help_tour' }).success).toBe(false)
    expect(eventSchemas.feature_wall_closed.safeParse({ dwell_ms: -1 }).success).toBe(false)
    expect(
      eventSchemas.feature_wall_closed.safeParse({
        dwell_ms: 1200,
        last_group_id: 'workbench',
        visited_substep_count: 12
      }).success
    ).toBe(false)
  })

  it('rejects private or unbounded close fields', () => {
    expect(
      eventSchemas.feature_wall_closed.safeParse({
        dwell_ms: 1200,
        repo_path: '/Users/alice/project'
      }).success
    ).toBe(false)
  })

  it('accepts only catalog tile ids', () => {
    expect(featureWallTileIdSchema.safeParse('tile-01').success).toBe(true)
    expect(featureWallTileIdSchema.safeParse('tile-99').success).toBe(false)
    expect(eventSchemas.feature_wall_tile_focused.safeParse({ tile_id: 'tile-12' }).success).toBe(
      true
    )
    expect(eventSchemas.feature_wall_tile_clicked.safeParse({ tile_id: 'tile-99' }).success).toBe(
      false
    )
  })

  it('accepts lifecycle workflow selection events', () => {
    expect(
      eventSchemas.feature_wall_group_selected.safeParse({
        group_id: 'scale',
        source: 'help_menu'
      }).success
    ).toBe(true)
    expect(
      eventSchemas.feature_wall_feature_selected.safeParse({
        group_id: 'build',
        tile_id: 'tile-04',
        source: 'help_menu'
      }).success
    ).toBe(true)
    expect(
      eventSchemas.feature_wall_docs_clicked.safeParse({
        group_id: 'ship',
        tile_id: 'tile-08',
        source: 'help_menu'
      }).success
    ).toBe(true)
  })
})
