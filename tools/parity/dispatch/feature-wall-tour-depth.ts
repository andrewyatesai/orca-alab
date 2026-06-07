// TS dispatch for the feature-wall-tour-depth parity module: maps the shared
// vector function names to the real `src/shared/feature-wall-tour-depth.ts`
// exports so the harness compares the live TS reference against the Rust port.

import type { AgentsStepId } from '../../../src/shared/agents-orchestration-steps'
import type { FeatureWallWorkflowId } from '../../../src/shared/feature-wall-workflows'
import type { ReviewStepId } from '../../../src/shared/review-steps'
import type { WorkbenchStepId } from '../../../src/shared/workbench-steps'
import {
  buildFeatureWallTourDepthSummary,
  getFeatureWallTourDepthStep,
  type FeatureWallTourDepthInput
} from '../../../src/shared/feature-wall-tour-depth'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'getFeatureWallTourDepthStep':
      return getFeatureWallTourDepthStep(
        input as {
          workflowId: FeatureWallWorkflowId
          agentStepId?: AgentsStepId
          workbenchStepId?: WorkbenchStepId
          reviewStepId?: ReviewStepId
        }
      )
    case 'buildFeatureWallTourDepthSummary': {
      // The vector carries visited sets as arrays and done records as plain
      // objects; rebuild the Set/Record shape the real function expects.
      const raw = input as {
        visitedWorkflows: FeatureWallWorkflowId[]
        visitedAgentSteps: AgentsStepId[]
        visitedWorkbenchSteps: WorkbenchStepId[]
        visitedReviewSteps: ReviewStepId[]
        workflowDone: FeatureWallTourDepthInput['workflowDone']
        agentStepDone: FeatureWallTourDepthInput['agentStepDone']
        workbenchStepDone: FeatureWallTourDepthInput['workbenchStepDone']
        reviewStepDone: FeatureWallTourDepthInput['reviewStepDone']
        lastGroupId: FeatureWallWorkflowId | null
      }
      return buildFeatureWallTourDepthSummary({
        visitedWorkflows: new Set(raw.visitedWorkflows),
        visitedAgentSteps: new Set(raw.visitedAgentSteps),
        visitedWorkbenchSteps: new Set(raw.visitedWorkbenchSteps),
        visitedReviewSteps: new Set(raw.visitedReviewSteps),
        workflowDone: raw.workflowDone,
        agentStepDone: raw.agentStepDone,
        workbenchStepDone: raw.workbenchStepDone,
        reviewStepDone: raw.reviewStepDone,
        lastGroupId: raw.lastGroupId
      })
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
