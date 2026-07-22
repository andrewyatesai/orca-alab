import {
  FEATURE_WALL_STEP_IDS,
  FEATURE_WALL_WORKFLOWS,
  type FeatureWallStepId,
  type FeatureWallWorkflowId
} from '../../../../shared/feature-wall-workflows'

export type FeatureWallCompletionProgress = {
  workflowDone: Record<FeatureWallWorkflowId, boolean>
  stepDone: Record<FeatureWallStepId, boolean>
}

export type FeatureWallCompletionProgressInput = {
  visitedWorkflows: ReadonlySet<FeatureWallWorkflowId>
  visitedSteps: ReadonlySet<FeatureWallStepId>
}

export function getFeatureWallCompletionProgress(
  input: FeatureWallCompletionProgressInput
): FeatureWallCompletionProgress {
  const stepDone = Object.fromEntries(
    FEATURE_WALL_STEP_IDS.map((stepId) => [stepId, input.visitedSteps.has(stepId)])
  ) as Record<FeatureWallStepId, boolean>
  const workflowDone = Object.fromEntries(
    FEATURE_WALL_WORKFLOWS.map((workflow) => [
      workflow.id,
      input.visitedWorkflows.has(workflow.id) &&
        workflow.steps.every((step) => input.visitedSteps.has(step.id))
    ])
  ) as Record<FeatureWallWorkflowId, boolean>

  return { workflowDone, stepDone }
}
