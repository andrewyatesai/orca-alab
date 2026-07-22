// Feature-wall tour-depth enum data + types. The behavior is owned by the Rust
// orca_core port and reached through the renderer's orca-git wasm adapter.
import type { FeatureWallStepId, FeatureWallWorkflowId } from './feature-wall-workflows'

export const FEATURE_WALL_TOUR_DEPTH_STEPS = [
  'terminal',
  'add-project',
  'tasks',
  'workspaces',
  'agents',
  'workbench',
  'browser-design',
  'review-ship',
  'cli-skills',
  'orchestration',
  'automations',
  'remote-mobile',
  'mobile-emulators',
  'computer-use'
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
  visitedSteps: ReadonlySet<FeatureWallStepId>
  workflowDone: Record<FeatureWallWorkflowId, boolean>
  stepDone: Record<FeatureWallStepId, boolean>
  lastGroupId: FeatureWallWorkflowId | null
}
