// Renderer feature-wall depth builders driven by the Rust orca_core port in
// orca-git wasm. Pre-ready summaries degrade to zero rather than guessing.
import { isGitWasmReady } from './git-line-stats'
import { orcaDispatch } from './orca_git_wasm.js'
import type {
  FeatureWallStepId,
  FeatureWallWorkflowId
} from '../../../../shared/feature-wall-workflows'
import type {
  FeatureWallTourDepthInput,
  FeatureWallTourDepthStep,
  FeatureWallTourDepthSummary
} from '../../../../shared/feature-wall-tour-depth'

function op(fn: string, input: unknown): unknown | null {
  if (!isGitWasmReady()) {
    return null
  }
  return JSON.parse(orcaDispatch('feature-wall-tour-depth', fn, JSON.stringify(input ?? null)))
}

export function getFeatureWallTourDepthStep(input: {
  workflowId: FeatureWallWorkflowId
  stepId?: FeatureWallStepId
}): FeatureWallTourDepthStep {
  const result = op('getFeatureWallTourDepthStep', input) as FeatureWallTourDepthStep | null
  return result ?? 'terminal'
}

export function buildFeatureWallTourDepthSummary(
  input: FeatureWallTourDepthInput
): FeatureWallTourDepthSummary {
  // Why: Sets serialize as empty objects, so flatten them before the JSON-only
  // wasm boundary used by both renderer telemetry and parity tests.
  const result = op('buildFeatureWallTourDepthSummary', {
    visitedWorkflows: [...input.visitedWorkflows],
    visitedSteps: [...input.visitedSteps],
    workflowDone: input.workflowDone,
    stepDone: input.stepDone,
    lastGroupId: input.lastGroupId
  }) as FeatureWallTourDepthSummary | null
  return (
    result ?? {
      visited_workflow_count: 0,
      visited_substep_count: 0,
      completed_workflow_count: 0,
      completed_substep_count: 0
    }
  )
}
