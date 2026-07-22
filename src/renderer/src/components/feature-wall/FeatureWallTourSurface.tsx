import { useCallback, useEffect, useId, useMemo, useRef, useState } from 'react'
import type { JSX, ReactNode } from 'react'
import {
  DEFAULT_FEATURE_WALL_STEP_ID,
  DEFAULT_FEATURE_WALL_WORKFLOW_ID,
  getFeatureWallMediaTile,
  type FeatureWallStepId,
  type FeatureWallWorkflow,
  type FeatureWallWorkflowId
} from '../../../../shared/feature-wall-workflows'
import type { FeatureWallOpenSourceTelemetry } from '../../../../shared/telemetry-events'
import type { FeatureWallTourDepthSummary } from '../../../../shared/feature-wall-tour-depth'
import { Button } from '@/components/ui/button'
import { translate } from '@/i18n/i18n'
import { getScreenSubmitModifierLabel } from '@/lib/screen-submit-shortcut'
import { track } from '@/lib/telemetry'
import { usePrefersReducedMotion } from './feature-wall-modal-helpers'
import { FeatureWallContinueButton } from './FeatureWallContinueButton'
import { getLocalizedFeatureWallWorkflows } from './feature-wall-localized-workflows'
import { focusVisibleFeatureWallStep, hasFocusedFeatureWallStep } from './feature-wall-step-focus'
import { FeatureWallTourPanel } from './FeatureWallTourPanel'
import { useFeatureWallCompletion } from './use-feature-wall-completion'
import { useFeatureWallTourKeyboardShortcut } from './use-feature-wall-tour-keyboard-shortcut'
import { useFeatureWallTourRailKeydown } from './use-feature-wall-tour-rail-keydown'
import { useFeatureWallTourTelemetry } from './use-feature-wall-tour-telemetry'

type FeatureWallTourSurfaceProps = {
  isOpen: boolean
  source: FeatureWallOpenSourceTelemetry
  onDone: (markSuccessfulExit?: () => void) => boolean | void | Promise<boolean | void>
  className?: string
  panelClassName?: string
  doneLabel?: string
  footerText?: string | null
  enableKeyboardShortcut?: boolean
  compactRail?: boolean
  detachedFooter?: boolean
  leadingFooterContent?: ReactNode
  finalSecondaryLabel?: string
  onFinalSecondaryAction?: () => void
  onTourDepthSummaryChange?: (summary: FeatureWallTourDepthSummary) => void
}

export function FeatureWallTourSurface({
  isOpen,
  source,
  onDone,
  className,
  panelClassName,
  doneLabel,
  footerText,
  enableKeyboardShortcut = true,
  compactRail = false,
  detachedFooter = false,
  leadingFooterContent,
  finalSecondaryLabel,
  onFinalSecondaryAction,
  onTourDepthSummaryChange
}: FeatureWallTourSurfaceProps): JSX.Element | null {
  const workflows = getLocalizedFeatureWallWorkflows()
  const resolvedDoneLabel =
    doneLabel ?? translate('auto.components.feature.wall.FeatureWallTourSurface.a120000003', 'Done')
  const resolvedFooterText =
    footerText === undefined
      ? translate(
          'auto.components.feature.wall.FeatureWallTourSurface.a120000004',
          'Reopen any time from Help > Explore Orca.'
        )
      : footerText
  const prefersReducedMotion = usePrefersReducedMotion()
  const reactId = useId()
  const previewPanelId = `${reactId}-feature-wall-preview-panel`
  const [selectedWorkflowId, setSelectedWorkflowId] = useState<FeatureWallWorkflowId>(
    DEFAULT_FEATURE_WALL_WORKFLOW_ID
  )
  const [selectedStepId, setSelectedStepId] = useState<FeatureWallStepId>(
    DEFAULT_FEATURE_WALL_STEP_ID
  )
  const railRefs = useRef<(HTMLButtonElement | null)[]>([])
  const selectedWorkflowIndex = useMemo(
    () =>
      Math.max(
        0,
        workflows.findIndex((workflow) => workflow.id === selectedWorkflowId)
      ),
    [selectedWorkflowId, workflows]
  )
  const selectedWorkflow = workflows[selectedWorkflowIndex]
  const activeStepIndex = Math.max(
    0,
    selectedWorkflow.steps.findIndex((step) => step.id === selectedStepId)
  )
  const activeStep = selectedWorkflow.steps[activeStepIndex]
  const completion = useFeatureWallCompletion(isOpen, { onTourDepthSummaryChange })
  const { markWorkflowVisited, markStepVisited } = completion
  const { markExitAction } = useFeatureWallTourTelemetry({
    isOpen,
    source,
    getDepthSummary: completion.getTourDepthSummary
  })
  const markWorkflowVisitedRef = useRef(markWorkflowVisited)
  const markStepVisitedRef = useRef(markStepVisited)
  markWorkflowVisitedRef.current = markWorkflowVisited
  markStepVisitedRef.current = markStepVisited

  useEffect(() => {
    if (!isOpen) {
      return
    }
    markWorkflowVisitedRef.current(DEFAULT_FEATURE_WALL_WORKFLOW_ID)
    markStepVisitedRef.current(DEFAULT_FEATURE_WALL_STEP_ID)
    trackWorkflowSelection(workflows[0], source)
  }, [isOpen, source, workflows])

  const handleSelectStep = useCallback(
    (workflow: FeatureWallWorkflow, stepId: FeatureWallStepId): void => {
      const transferRailFocus = hasFocusedFeatureWallStep()
      markWorkflowVisited(workflow.id)
      markStepVisited(stepId)
      setSelectedWorkflowId(workflow.id)
      setSelectedStepId(stepId)
      // Why: shortcut navigation can hide the focused step row at a chapter
      // boundary; keep keyboard focus aligned with the newly selected screen.
      if (transferRailFocus) {
        focusVisibleFeatureWallStep(stepId)
      }
    },
    [markStepVisited, markWorkflowVisited]
  )

  const handleSelectWorkflow = useCallback(
    (workflow: FeatureWallWorkflow): void => {
      const firstStep = workflow.steps[0]
      if (!firstStep) {
        return
      }
      handleSelectStep(workflow, firstStep.id)
      trackWorkflowSelection(workflow, source)
    },
    [handleSelectStep, source]
  )

  const handleRailKeyDown = useFeatureWallTourRailKeydown({
    railRefs,
    onSelectWorkflow: handleSelectWorkflow
  })
  const isLastWorkflow = selectedWorkflowIndex === workflows.length - 1
  const isLastStep = activeStepIndex === selectedWorkflow.steps.length - 1
  const isTourEnd = isLastWorkflow && isLastStep
  const handleDone = useCallback((): void => {
    const exitAction = source === 'onboarding' ? 'onboarding_continue' : 'done'
    let markedSuccessfulExit = false
    const markSuccessfulExit = (): void => {
      if (!markedSuccessfulExit) {
        markedSuccessfulExit = true
        markExitAction(exitAction)
      }
    }
    const doneResult = onDone(markSuccessfulExit)
    if (doneResult instanceof Promise) {
      void doneResult.then((result) => result !== false && markSuccessfulExit())
    } else if (doneResult !== false) {
      markSuccessfulExit()
    }
  }, [markExitAction, onDone, source])

  const handleFinalSecondaryAction = useCallback((): void => {
    markExitAction(source === 'onboarding' ? 'onboarding_continue' : 'done')
    onFinalSecondaryAction?.()
  }, [markExitAction, onFinalSecondaryAction, source])

  const handleContinue = useCallback((): void => {
    handleSelectStep(selectedWorkflow, activeStep.id)
    const nextStep = selectedWorkflow.steps[activeStepIndex + 1]
    if (nextStep) {
      handleSelectStep(selectedWorkflow, nextStep.id)
      return
    }
    const nextWorkflow = workflows[selectedWorkflowIndex + 1]
    if (nextWorkflow) {
      handleSelectWorkflow(nextWorkflow)
      return
    }
    handleDone()
  }, [
    activeStep.id,
    activeStepIndex,
    handleDone,
    handleSelectStep,
    handleSelectWorkflow,
    selectedWorkflow,
    selectedWorkflowIndex,
    workflows
  ])

  const handleBack = useCallback((): void => {
    const previousStep = selectedWorkflow.steps[activeStepIndex - 1]
    if (previousStep) {
      handleSelectStep(selectedWorkflow, previousStep.id)
      return
    }
    const previousWorkflow = workflows[selectedWorkflowIndex - 1]
    const previousWorkflowLastStep = previousWorkflow?.steps.at(-1)
    if (previousWorkflow && previousWorkflowLastStep) {
      handleSelectStep(previousWorkflow, previousWorkflowLastStep.id)
      trackWorkflowSelection(previousWorkflow, source)
    }
  }, [
    activeStepIndex,
    handleSelectStep,
    selectedWorkflow,
    selectedWorkflowIndex,
    source,
    workflows
  ])

  useFeatureWallTourKeyboardShortcut({
    isOpen,
    enabled: enableKeyboardShortcut,
    onContinue: handleContinue
  })

  if (!isOpen) {
    return null
  }

  const previewTitleId = `${reactId}-feature-wall-preview-${activeStep.id}`
  const continueButton = (
    <div className="flex items-center gap-2">
      {isTourEnd && finalSecondaryLabel && onFinalSecondaryAction ? (
        <Button type="button" variant="outline" size="sm" onClick={handleFinalSecondaryAction}>
          {finalSecondaryLabel}
        </Button>
      ) : null}
      <FeatureWallContinueButton
        label={
          isTourEnd
            ? resolvedDoneLabel
            : translate(
                'auto.components.feature.wall.FeatureWallTourSurface.a110000001',
                'Continue'
              )
        }
        enableKeyboardShortcut={enableKeyboardShortcut}
        shortcutModifierLabel={getScreenSubmitModifierLabel()}
        onClick={handleContinue}
      />
    </div>
  )
  const isTourStart = selectedWorkflowIndex === 0 && activeStepIndex === 0
  const defaultLeadingContent = isTourStart ? null : (
    <Button type="button" variant="ghost" size="sm" onClick={handleBack}>
      {translate('auto.components.feature.wall.FeatureWallTourSurface.a110000002', 'Back')}
    </Button>
  )
  const resolvedLeadingContent = leadingFooterContent ?? defaultLeadingContent

  return (
    <FeatureWallTourPanel
      className={className}
      panelClassName={panelClassName}
      detachedFooter={detachedFooter}
      compactRail={compactRail}
      previewPanelId={previewPanelId}
      previewTitleId={previewTitleId}
      selectedWorkflow={selectedWorkflow}
      activeStep={activeStep}
      source={source}
      completion={completion}
      railRefs={railRefs}
      onSelectWorkflow={handleSelectWorkflow}
      onSelectStep={handleSelectStep}
      onRailKeyDown={handleRailKeyDown}
      prefersReducedMotion={prefersReducedMotion}
      footerText={resolvedFooterText}
      continueButton={continueButton}
      leadingFooterContent={resolvedLeadingContent}
    />
  )
}

function trackWorkflowSelection(
  workflow: FeatureWallWorkflow,
  source: FeatureWallOpenSourceTelemetry
): void {
  track('feature_wall_group_selected', { group_id: workflow.id, source })
  const tile = getFeatureWallMediaTile(workflow.primaryTileId)
  if (!tile) {
    return
  }
  track('feature_wall_feature_selected', {
    group_id: workflow.id,
    tile_id: tile.id,
    source
  })
  track('feature_wall_tile_focused', { tile_id: tile.id })
}
