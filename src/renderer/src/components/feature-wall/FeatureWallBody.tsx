import type { JSX } from 'react'
import type { FeatureWallStep } from '../../../../shared/feature-wall-workflows'
import { FeatureWallStepVisual } from './FeatureWallStepVisual'

export function FeatureWallBody(props: {
  activeStep: FeatureWallStep
  prefersReducedMotion: boolean
}): JSX.Element {
  return (
    <div className="flex min-h-full flex-col px-8 pb-4 pt-1 [@media(max-height:500px)]:px-3 [@media(max-height:500px)]:pb-2 [@media(max-height:500px)]:pt-0">
      <FeatureWallStepVisual
        stepId={props.activeStep.id}
        reducedMotion={props.prefersReducedMotion}
      />
    </div>
  )
}
