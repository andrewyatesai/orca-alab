import { useCallback, useState } from 'react'
import type { Dispatch, SetStateAction } from 'react'
import type {
  FeatureWallStepId,
  FeatureWallWorkflowId
} from '../../../../shared/feature-wall-workflows'
import {
  persistVisitedStep,
  persistVisitedWorkflow,
  readPersistedVisitedSteps,
  readPersistedVisitedWorkflows
} from './feature-wall-completion-persistence'

export type PersistedFeatureWallCompletionState = {
  visitedWorkflows: Set<FeatureWallWorkflowId>
  visitedSteps: Set<FeatureWallStepId>
  markWorkflowVisited: (id: FeatureWallWorkflowId) => void
  markStepVisited: (id: FeatureWallStepId) => void
}

function addToSet<T>(setValue: Dispatch<SetStateAction<Set<T>>>, id: T): void {
  setValue((previous) => {
    if (previous.has(id)) {
      return previous
    }
    const next = new Set(previous)
    next.add(id)
    return next
  })
}

export function usePersistedFeatureWallCompletion(): PersistedFeatureWallCompletionState {
  const [visitedWorkflows, setVisitedWorkflows] = useState<Set<FeatureWallWorkflowId>>(() =>
    readPersistedVisitedWorkflows()
  )
  const [visitedSteps, setVisitedSteps] = useState<Set<FeatureWallStepId>>(() =>
    readPersistedVisitedSteps()
  )

  const markWorkflowVisited = useCallback((id: FeatureWallWorkflowId): void => {
    persistVisitedWorkflow(id)
    addToSet(setVisitedWorkflows, id)
  }, [])
  const markStepVisited = useCallback((id: FeatureWallStepId): void => {
    persistVisitedStep(id)
    addToSet(setVisitedSteps, id)
  }, [])

  return {
    visitedWorkflows,
    visitedSteps,
    markWorkflowVisited,
    markStepVisited
  }
}
