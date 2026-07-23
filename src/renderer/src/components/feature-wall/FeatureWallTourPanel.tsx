import { useState } from 'react'
import type { JSX, KeyboardEvent, MutableRefObject, ReactNode } from 'react'
import { ArrowUpRight, ChevronDown } from 'lucide-react'
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
import { useFeatureWallTourPanelScroll } from './use-feature-wall-tour-panel-scroll'

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
  const descriptionId = `${props.previewTitleId}-description`
  const { panelRef, contentRef, hasMoreContent, handleScroll, scrollForward } =
    useFeatureWallTourPanelScroll({
      activeStepId: props.activeStep.id,
      prefersReducedMotion: props.prefersReducedMotion
    })
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

        <div className="relative min-h-0 overflow-hidden">
          <section
            ref={panelRef}
            id={props.previewPanelId}
            role="tabpanel"
            tabIndex={0}
            onScroll={handleScroll}
            className="scrollbar-sleek h-full min-h-0 overflow-y-auto outline-none focus-visible:ring-[3px] focus-visible:ring-inset focus-visible:ring-ring/50"
            aria-labelledby={`${props.previewPanelId}-workflow-${props.selectedWorkflow.id} ${props.previewTitleId}`}
            aria-describedby={descriptionId}
          >
            <div ref={contentRef} className="grid min-h-full grid-rows-[auto_minmax(0,1fr)]">
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
                  'px-4 pb-2 pt-3 text-center [@media(max-height:500px)]:px-3 [@media(max-height:500px)]:pb-0 [@media(max-height:500px)]:pt-2 md:px-8 md:pt-4'
                )}
              >
                <p className="flex flex-wrap items-center justify-center gap-1.5 text-xs font-medium text-muted-foreground [@media(max-height:500px)]:sr-only">
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
                    className="text-xl font-semibold leading-tight tracking-tight [@media(max-height:500px)]:text-base md:text-2xl"
                  >
                    {props.activeStep.title}
                  </h3>
                  {props.activeStep.availabilityLabel ? (
                    <span className="rounded-full border border-border bg-background px-2 py-0.5 text-[11px] font-medium text-muted-foreground">
                      {props.activeStep.availabilityLabel}
                    </span>
                  ) : null}
                </div>
                <FeatureWallStepDescription
                  key={props.activeStep.id}
                  id={descriptionId}
                  description={props.activeStep.description}
                />
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
            </div>
          </section>
          {hasMoreContent ? (
            <div className="pointer-events-none absolute inset-x-0 bottom-0 z-20 flex h-16 items-end justify-center bg-gradient-to-t from-card via-card/90 to-transparent pb-3">
              <Button
                type="button"
                variant="outline"
                size="xs"
                className="pointer-events-auto rounded-full bg-card/95 px-3 shadow-xs"
                aria-controls={props.previewPanelId}
                data-feature-wall-scroll-affordance
                onClick={scrollForward}
              >
                {translate('auto.fw.walkthrough.moreBelow', 'More below')}
                <ChevronDown className="size-3" />
              </Button>
            </div>
          ) : null}
        </div>
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

function FeatureWallStepDescription(props: { id: string; description: string }): JSX.Element {
  const [expanded, setExpanded] = useState(false)
  const disclosureLabel = expanded
    ? translate('auto.fw.walkthrough.collapseDescription', 'Collapse description')
    : translate('auto.fw.walkthrough.expandDescription', 'Show full description')

  return (
    <>
      <p
        id={props.id}
        data-feature-wall-step-description
        className={cn(
          'mx-auto mt-2 max-w-[62ch] text-xs leading-relaxed text-muted-foreground [@media(max-height:500px)]:mt-1 [@media(max-height:500px)]:text-[11px] [@media(max-height:500px)]:leading-snug md:text-sm',
          !expanded && '[@media(max-height:500px)]:line-clamp-2'
        )}
      >
        {props.description}
      </p>
      <Button
        type="button"
        variant="ghost"
        size="xs"
        className="mx-auto mt-0.5 hidden text-muted-foreground [@media(max-height:500px)]:inline-flex"
        aria-expanded={expanded}
        aria-controls={props.id}
        onClick={() => setExpanded((value) => !value)}
      >
        {disclosureLabel}
        <ChevronDown
          className={cn(
            'size-3 transition-transform motion-reduce:transition-none',
            expanded && 'rotate-180'
          )}
        />
      </Button>
    </>
  )
}
