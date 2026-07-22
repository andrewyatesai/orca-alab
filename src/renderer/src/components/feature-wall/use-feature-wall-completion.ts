import { useCallback, useMemo } from 'react'
import type {
  FeatureWallStepId,
  FeatureWallWorkflowId
} from '../../../../shared/feature-wall-workflows'
import type { FeatureWallTourDepthSummary } from '../../../../shared/feature-wall-tour-depth'
import { getFeatureWallCompletionProgress } from './feature-wall-completion-progress'
import { usePersistedFeatureWallCompletion } from './use-persisted-feature-wall-completion'
import { useFeatureWallSessionDepth } from './use-feature-wall-session-depth'

export type FeatureWallCompletionState = {
  workflowDone: Record<FeatureWallWorkflowId, boolean>
  stepDone: Record<FeatureWallStepId, boolean>
  markWorkflowVisited: (id: FeatureWallWorkflowId) => void
  markStepVisited: (id: FeatureWallStepId) => void
  getTourDepthSummary: () => FeatureWallTourDepthSummary
}

export function useFeatureWallCompletion(
  isOpen: boolean,
  options: { onTourDepthSummaryChange?: (summary: FeatureWallTourDepthSummary) => void } = {}
): FeatureWallCompletionState {
  const persisted = usePersistedFeatureWallCompletion()
  const sessionDepth = useFeatureWallSessionDepth({
    isOpen,
    onTourDepthSummaryChange: options.onTourDepthSummaryChange
  })
  const {
    visitedWorkflows,
    visitedSteps,
    markWorkflowVisited: persistWorkflowVisited,
    markStepVisited: persistStepVisited
  } = persisted
  const { markWorkflowVisitedForSession, markStepVisitedForSession, getTourDepthSummary } =
    sessionDepth
  const progress = useMemo(
    () =>
      getFeatureWallCompletionProgress({
        visitedWorkflows,
        visitedSteps
      }),
    [visitedSteps, visitedWorkflows]
  )

  const markWorkflowVisited = useCallback(
    (id: FeatureWallWorkflowId): void => {
      persistWorkflowVisited(id)
      markWorkflowVisitedForSession(id)
    },
    [markWorkflowVisitedForSession, persistWorkflowVisited]
  )
  const markStepVisited = useCallback(
    (id: FeatureWallStepId): void => {
      persistStepVisited(id)
      markStepVisitedForSession(id)
    },
    [markStepVisitedForSession, persistStepVisited]
  )

  return {
    ...progress,
    markWorkflowVisited,
    markStepVisited,
    getTourDepthSummary
  }
}
