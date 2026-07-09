// Renderer feature-wall tour-depth telemetry builders, driven by the Rust
// `orca_core::feature_wall_tour_depth` port in the orca-git wasm (the shared TS
// bodies were deleted; the enum data + types still live in TS). Every build goes
// through the single `op` JSON boundary. Pre-ready the op returns null, so the
// summary degrades to an all-zero depth (never a mis-counted payload) during the
// ~tens-of-ms wasm boot window — the sole consumer emits telemetry off a valid
// summary either way.
import { isGitWasmReady } from './git-line-stats'
import { orcaDispatch } from './orca_git_wasm.js'
import type { AgentsStepId } from '../../../../shared/agents-orchestration-steps'
import type { FeatureWallWorkflowId } from '../../../../shared/feature-wall-workflows'
import type {
  FeatureWallTourDepthInput,
  FeatureWallTourDepthStep,
  FeatureWallTourDepthSummary
} from '../../../../shared/feature-wall-tour-depth'
import type { ReviewStepId } from '../../../../shared/review-steps'
import type { WorkbenchStepId } from '../../../../shared/workbench-steps'

function op(fn: string, input: unknown): unknown | null {
  if (!isGitWasmReady()) {
    return null
  }
  return JSON.parse(orcaDispatch('feature-wall-tour-depth', fn, JSON.stringify(input ?? null)))
}

export function getFeatureWallTourDepthStep(input: {
  workflowId: FeatureWallWorkflowId
  agentStepId?: AgentsStepId
  workbenchStepId?: WorkbenchStepId
  reviewStepId?: ReviewStepId
}): FeatureWallTourDepthStep {
  const r = op('getFeatureWallTourDepthStep', input) as FeatureWallTourDepthStep | null
  // No in-app consumer (parity-only); pre-ready fall back to the first canonical step.
  return r ?? 'workspaces'
}

export function buildFeatureWallTourDepthSummary(
  input: FeatureWallTourDepthInput
): FeatureWallTourDepthSummary {
  // Rust reads the visited sets as JSON arrays; JSON.stringify(Set) yields `{}`,
  // so spread each Set to an array before crossing the boundary.
  const r = op('buildFeatureWallTourDepthSummary', {
    visitedWorkflows: [...input.visitedWorkflows],
    visitedAgentSteps: [...input.visitedAgentSteps],
    visitedWorkbenchSteps: [...input.visitedWorkbenchSteps],
    visitedReviewSteps: [...input.visitedReviewSteps],
    workflowDone: input.workflowDone,
    agentStepDone: input.agentStepDone,
    workbenchStepDone: input.workbenchStepDone,
    reviewStepDone: input.reviewStepDone,
    lastGroupId: input.lastGroupId
  }) as FeatureWallTourDepthSummary | null
  return (
    r ?? {
      visited_workflow_count: 0,
      visited_substep_count: 0,
      completed_workflow_count: 0,
      completed_substep_count: 0
    }
  )
}
