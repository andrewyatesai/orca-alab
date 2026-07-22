import { useCallback } from 'react'
import type { KeyboardEvent, RefObject } from 'react'
import {
  FEATURE_WALL_WORKFLOWS,
  type FeatureWallWorkflow
} from '../../../../shared/feature-wall-workflows'
import {
  getFeatureWallRailNavigationTarget,
  normalizeFeatureWallRailNavigationKey,
  type FeatureWallRailOrientation
} from './feature-wall-rail-navigation'

export function useFeatureWallTourRailKeydown({
  railRefs,
  onSelectWorkflow
}: {
  railRefs: RefObject<(HTMLButtonElement | null)[]>
  onSelectWorkflow: (workflow: FeatureWallWorkflow) => void
}): (
  event: KeyboardEvent<HTMLButtonElement>,
  index: number,
  orientation: FeatureWallRailOrientation
) => void {
  return useCallback(
    (
      event: KeyboardEvent<HTMLButtonElement>,
      index: number,
      orientation: FeatureWallRailOrientation
    ): void => {
      const navigationKey = normalizeFeatureWallRailNavigationKey(event.key, orientation)
      if (!navigationKey) {
        return
      }
      event.preventDefault()
      const nextIndex = getFeatureWallRailNavigationTarget({
        currentIndex: index,
        key: navigationKey,
        itemCount: FEATURE_WALL_WORKFLOWS.length
      })
      const nextWorkflow = FEATURE_WALL_WORKFLOWS[nextIndex]
      if (!nextWorkflow) {
        return
      }
      onSelectWorkflow(nextWorkflow)
      railRefs.current[nextIndex]?.focus()
    },
    [onSelectWorkflow, railRefs]
  )
}
