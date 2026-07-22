import type { FeatureWallStepId } from '../../../../shared/feature-wall-workflows'

const STEP_SELECTOR = '[data-feature-wall-step-id]'

export function hasFocusedFeatureWallStep(): boolean {
  return (
    document.activeElement instanceof HTMLElement && document.activeElement.matches(STEP_SELECTOR)
  )
}

export function focusVisibleFeatureWallStep(stepId: FeatureWallStepId): void {
  window.requestAnimationFrame(() => {
    const candidates = document.querySelectorAll<HTMLElement>(
      `[data-feature-wall-step-id="${stepId}"]`
    )
    const visible = [...candidates].find((candidate) => candidate.offsetParent !== null)
    visible?.focus()
  })
}
