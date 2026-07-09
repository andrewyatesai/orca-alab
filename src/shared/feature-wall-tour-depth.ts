// Feature-wall tour-depth enum data + types. The behavior (getFeatureWallTourDepthStep,
// buildFeatureWallTourDepthSummary) was cut over to the Rust
// `orca_core::feature_wall_tour_depth` port — the renderer drives it via the
// orca-git wasm (see src/renderer/src/lib/git-wasm/feature-wall-tour-depth.ts).
// The two enum consts stay in TS because telemetry-events.ts builds z.enum()
// schemas from them; this file must remain import-safe on every surface (no
// napi/wasm), so it carries data + types only.
import type { AgentsStepId } from './agents-orchestration-steps'
import type { FeatureWallWorkflowId } from './feature-wall-workflows'
import type { ReviewStepId } from './review-steps'
import type { WorkbenchStepId } from './workbench-steps'

export const FEATURE_WALL_TOUR_DEPTH_STEPS = [
  'workspaces',
  'tasks',
  'agents_statuses',
  'agents_usage',
  'agents_orchestration',
  'workbench_terminal',
  'workbench_editor',
  'workbench_browser',
  'review_notes',
  'review_pr_view',
  'review_ship'
] as const

export type FeatureWallTourDepthStep = (typeof FEATURE_WALL_TOUR_DEPTH_STEPS)[number]

export const FEATURE_WALL_EXIT_ACTIONS = ['done', 'dismissed', 'onboarding_continue'] as const

export type FeatureWallExitAction = (typeof FEATURE_WALL_EXIT_ACTIONS)[number]

export type FeatureWallTourDepthSummary = {
  furthest_step?: FeatureWallTourDepthStep
  last_group_id?: FeatureWallWorkflowId
  visited_workflow_count: number
  visited_substep_count: number
  completed_workflow_count: number
  completed_substep_count: number
}

export type FeatureWallTourDepthInput = {
  visitedWorkflows: ReadonlySet<FeatureWallWorkflowId>
  visitedAgentSteps: ReadonlySet<AgentsStepId>
  visitedWorkbenchSteps: ReadonlySet<WorkbenchStepId>
  visitedReviewSteps: ReadonlySet<ReviewStepId>
  workflowDone: Record<FeatureWallWorkflowId, boolean>
  agentStepDone: Record<AgentsStepId, boolean>
  workbenchStepDone: Record<WorkbenchStepId, boolean>
  reviewStepDone: Record<ReviewStepId, boolean>
  lastGroupId: FeatureWallWorkflowId | null
}
