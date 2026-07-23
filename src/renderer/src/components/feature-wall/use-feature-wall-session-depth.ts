import { useCallback, useEffect, useRef } from 'react'
import type {
  FeatureWallStepId,
  FeatureWallWorkflowId
} from '../../../../shared/feature-wall-workflows'
import type { FeatureWallTourDepthSummary } from '../../../../shared/feature-wall-tour-depth'
import { buildFeatureWallTourDepthSummary } from '@/lib/git-wasm/feature-wall-tour-depth'
import { getFeatureWallCompletionProgress } from './feature-wall-completion-progress'

type FeatureWallSessionDepthInput = {
  isOpen: boolean
  onTourDepthSummaryChange?: (summary: FeatureWallTourDepthSummary) => void
}

export type FeatureWallSessionDepthTracker = {
  markWorkflowVisitedForSession: (id: FeatureWallWorkflowId) => void
  markStepVisitedForSession: (id: FeatureWallStepId) => void
  getTourDepthSummary: () => FeatureWallTourDepthSummary
}

type SessionDepthState = {
  visitedWorkflows: Set<FeatureWallWorkflowId>
  visitedSteps: Set<FeatureWallStepId>
  lastGroupId: FeatureWallWorkflowId | null
}

function createEmptySessionDepth(): SessionDepthState {
  return {
    visitedWorkflows: new Set(),
    visitedSteps: new Set(),
    lastGroupId: null
  }
}

export function useFeatureWallSessionDepth(
  input: FeatureWallSessionDepthInput
): FeatureWallSessionDepthTracker {
  const { isOpen, onTourDepthSummaryChange } = input
  const sessionDepthRef = useRef<SessionDepthState>(createEmptySessionDepth())

  const getTourDepthSummary = useCallback((): FeatureWallTourDepthSummary => {
    const session = sessionDepthRef.current
    const progress = getFeatureWallCompletionProgress(session)
    return buildFeatureWallTourDepthSummary({
      ...progress,
      visitedWorkflows: session.visitedWorkflows,
      visitedSteps: session.visitedSteps,
      lastGroupId: session.lastGroupId
    })
  }, [])

  const publishTourDepthSummary = useCallback((): void => {
    onTourDepthSummaryChange?.(getTourDepthSummary())
  }, [getTourDepthSummary, onTourDepthSummaryChange])

  const wasOpenRef = useRef(false)
  useEffect(() => {
    if (!isOpen) {
      wasOpenRef.current = false
      return
    }
    if (!wasOpenRef.current) {
      // Why: close-depth telemetry describes this explicit replay, not progress
      // restored from a previous walkthrough session.
      sessionDepthRef.current = createEmptySessionDepth()
    }
    wasOpenRef.current = true
    publishTourDepthSummary()
  }, [isOpen, publishTourDepthSummary])

  const markWorkflowVisitedForSession = useCallback(
    (id: FeatureWallWorkflowId): void => {
      const session = sessionDepthRef.current
      session.lastGroupId = id
      session.visitedWorkflows.add(id)
      publishTourDepthSummary()
    },
    [publishTourDepthSummary]
  )
  const markStepVisitedForSession = useCallback(
    (id: FeatureWallStepId): void => {
      sessionDepthRef.current.visitedSteps.add(id)
      publishTourDepthSummary()
    },
    [publishTourDepthSummary]
  )

  return {
    markWorkflowVisitedForSession,
    markStepVisitedForSession,
    getTourDepthSummary
  }
}
