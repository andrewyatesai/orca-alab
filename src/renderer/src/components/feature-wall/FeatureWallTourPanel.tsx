import type { JSX, KeyboardEvent, MutableRefObject, ReactNode } from 'react'
import { ArrowUpRight } from 'lucide-react'
import {
  FEATURE_WALL_STEP_IDS,
  getFeatureWallMediaTile,
  type FeatureWallStep,
  type FeatureWallStepId,
  type FeatureWallWorkflow,
  type FeatureWallWorkflowId
} from '../../../../shared/feature-wall-workflows'
import type { FeatureWallOpenSourceTelemetry } from '../../../../shared/telemetry-events'
import { Button } from '@/components/ui/button'
import { translate } from '@/i18n/i18n'
import { track } from '@/lib/telemetry'
import { cn } from '@/lib/utils'
import type { FeatureWallCompletionState } from './use-feature-wall-completion'
import { FeatureWallBody } from './FeatureWallBody'
import { FeatureWallRail } from './FeatureWallRail'
import type { FeatureWallRailOrientation } from './feature-wall-rail-navigation'

export function FeatureWallTourPanel(props: {
  className?: string
  panelClassName?: string
  detachedFooter: boolean
  compactRail: boolean
  previewPanelId: string
  previewTitleId: string
  selectedWorkflow: FeatureWallWorkflow
  activeStep: FeatureWallStep
  source: FeatureWallOpenSourceTelemetry
  completion: FeatureWallCompletionState
  railRefs: MutableRefObject<(HTMLButtonElement | null)[]>
  onSelectWorkflow: (workflow: FeatureWallWorkflow) => void
  onSelectStep: (workflow: FeatureWallWorkflow, stepId: FeatureWallStepId) => void
  onRailKeyDown: (
    event: KeyboardEvent<HTMLButtonElement>,
    index: number,
    orientation: FeatureWallRailOrientation
  ) => void
  prefersReducedMotion: boolean
  footerText: string | null
  continueButton: ReactNode
  leadingFooterContent?: ReactNode
}): JSX.Element {
  const contentStageClassName = 'mx-auto w-full max-w-[940px]'
  const activeStepNumber = Math.max(1, FEATURE_WALL_STEP_IDS.indexOf(props.activeStep.id) + 1)
  const stepCount = FEATURE_WALL_STEP_IDS.length
  const stepProgressText = translate(
    'auto.components.feature.wall.FeatureWallTourPanel.a120000001',
    '{{value0}} of {{value1}}',
    { value0: activeStepNumber, value1: stepCount }
  )
  const openWorkflowDocs = (): void => {
    const tile = getFeatureWallMediaTile(props.selectedWorkflow.primaryTileId)
    if (tile) {
      track('feature_wall_docs_clicked', {
        group_id: props.selectedWorkflow.id,
        tile_id: tile.id,
        source: props.source
      })
      track('feature_wall_tile_clicked', { tile_id: tile.id })
    }
    void window.api.shell.openUrl(props.activeStep.docsUrl ?? props.selectedWorkflow.docsUrl)
  }
  const panel = (
    <div
      className={cn(
        'grid min-h-0 overflow-hidden',
        props.detachedFooter ? 'grid-rows-[minmax(0,1fr)]' : 'grid-rows-[minmax(0,1fr)_auto]',
        props.detachedFooter ? props.panelClassName : props.className
      )}
    >
      <div
        className={cn(
          'grid min-h-0 grid-rows-[auto_minmax(0,1fr)] md:grid-rows-1',
          props.compactRail
            ? 'md:grid-cols-[210px_minmax(0,1fr)] lg:grid-cols-[225px_minmax(0,1fr)]'
            : 'md:grid-cols-[260px_minmax(0,1fr)] lg:grid-cols-[280px_minmax(0,1fr)]'
        )}
      >
        <div className="min-h-0 md:border-r md:border-border">
          <FeatureWallRail
            selectedWorkflowId={props.selectedWorkflow.id as FeatureWallWorkflowId}
            selectedStepId={props.activeStep.id}
            previewPanelId={props.previewPanelId}
            railRefs={props.railRefs}
            onSelectWorkflow={props.onSelectWorkflow}
            onSelectStep={props.onSelectStep}
            onRailKeyDown={props.onRailKeyDown}
            workflowDone={props.completion.workflowDone}
            stepDone={props.completion.stepDone}
          />
        </div>

        <section
          id={props.previewPanelId}
          role="tabpanel"
          className="scrollbar-sleek grid min-h-0 grid-rows-[auto_minmax(0,1fr)] overflow-y-auto"
          aria-labelledby={`${props.previewPanelId}-workflow-${props.selectedWorkflow.id} ${props.previewTitleId}`}
        >
          <span className="sr-only" role="status" aria-live="polite" aria-atomic="true">
            {translate(
              'auto.components.feature.wall.FeatureWallTourPanel.a120000004',
              'Step {{value0}} of {{value1}}: {{value2}}',
              {
                value0: activeStepNumber,
                value1: stepCount,
                value2: props.activeStep.title
              }
            )}
          </span>
          <div
            className={cn(
              contentStageClassName,
              'px-4 pb-2 pt-3 text-center [@media(max-height:500px)]:px-3 [@media(max-height:500px)]:pb-0 [@media(max-height:500px)]:pt-2 md:px-8 md:pb-3 md:pt-6'
            )}
          >
            <p className="flex flex-wrap items-center justify-center gap-1.5 text-xs font-medium text-muted-foreground">
              <span>{props.selectedWorkflow.meta}</span>
              <span aria-hidden>·</span>
              <span
                role="progressbar"
                aria-label={translate(
                  'auto.components.feature.wall.FeatureWallTourPanel.a120000003',
                  'Tour progress'
                )}
                aria-valuemin={1}
                aria-valuemax={stepCount}
                aria-valuenow={activeStepNumber}
                aria-valuetext={stepProgressText}
              >
                {stepProgressText}
              </span>
            </p>
            <div className="mt-2 flex flex-wrap items-center justify-center gap-2">
              <h3
                id={props.previewTitleId}
                className="text-xl font-semibold leading-tight tracking-tight [@media(max-height:500px)]:text-lg md:text-2xl"
              >
                {props.activeStep.title}
              </h3>
              {props.activeStep.availabilityLabel ? (
                <span className="rounded-full border border-border bg-background px-2 py-0.5 text-[11px] font-medium text-muted-foreground">
                  {props.activeStep.availabilityLabel}
                </span>
              ) : null}
            </div>
            <p className="mx-auto mt-2 max-w-[62ch] text-xs leading-relaxed text-muted-foreground [@media(max-height:500px)]:mt-1 [@media(max-height:500px)]:text-[11px] [@media(max-height:500px)]:leading-snug md:mt-3 md:text-sm">
              {props.activeStep.description}
            </p>
            <Button
              type="button"
              variant="link"
              size="sm"
              className="mt-0.5 h-7 px-1 text-xs [@media(max-height:500px)]:h-6"
              aria-label={translate(
                'auto.components.feature.wall.FeatureWallTourPanel.a120000005',
                'Learn more about {{value0}}',
                { value0: props.activeStep.name }
              )}
              onClick={openWorkflowDocs}
            >
              {translate(
                'auto.components.feature.wall.FeatureWallTourPanel.a120000002',
                'Learn more'
              )}
              <ArrowUpRight className="size-3" />
            </Button>
          </div>

          <div className={contentStageClassName}>
            <FeatureWallBody
              activeStep={props.activeStep}
              prefersReducedMotion={props.prefersReducedMotion}
            />
          </div>
        </section>
      </div>

      {!props.detachedFooter ? (
        <footer className="flex items-center justify-between gap-2 border-t border-border bg-card/50 px-3 py-2 md:px-7 md:py-3">
          {props.leadingFooterContent ? (
            props.leadingFooterContent
          ) : props.footerText ? (
            <span className="text-xs text-muted-foreground">{props.footerText}</span>
          ) : (
            <span />
          )}
          {props.continueButton}
        </footer>
      ) : null}
    </div>
  )

  if (props.detachedFooter) {
    return (
      <div className={cn('grid min-h-0 grid-rows-[minmax(0,1fr)_auto] gap-3', props.className)}>
        {panel}
        <div className="flex items-center justify-between gap-3">
          {props.leadingFooterContent ?? <span />}
          {props.continueButton}
        </div>
      </div>
    )
  }

  return panel
}
