import { useEffect, useRef, useSyncExternalStore } from 'react'
import type { JSX, KeyboardEvent, MutableRefObject } from 'react'
import { Check } from 'lucide-react'
import type {
  FeatureWallStepId,
  FeatureWallWorkflow,
  FeatureWallWorkflowId
} from '../../../../shared/feature-wall-workflows'
import { cn } from '@/lib/utils'
import { translate } from '@/i18n/i18n'
import type { FeatureWallRailOrientation } from './feature-wall-rail-navigation'
import { getLocalizedFeatureWallWorkflows } from './feature-wall-localized-workflows'
import { focusVisibleFeatureWallStep, hasFocusedFeatureWallStep } from './feature-wall-step-focus'

const SUB_STEP_LABELS = ['a', 'b', 'c'] as const
const DESKTOP_RAIL_MEDIA_QUERY = '(min-width: 768px)'

export function FeatureWallRail(props: {
  selectedWorkflowId: FeatureWallWorkflowId
  selectedStepId: FeatureWallStepId
  previewPanelId: string
  railRefs: MutableRefObject<(HTMLButtonElement | null)[]>
  onSelectWorkflow: (workflow: FeatureWallWorkflow) => void
  onSelectStep: (workflow: FeatureWallWorkflow, stepId: FeatureWallStepId) => void
  onRailKeyDown: (
    event: KeyboardEvent<HTMLButtonElement>,
    index: number,
    orientation: FeatureWallRailOrientation
  ) => void
  workflowDone: Record<FeatureWallWorkflowId, boolean>
  stepDone: Record<FeatureWallStepId, boolean>
}): JSX.Element {
  const workflows = getLocalizedFeatureWallWorkflows()
  const orientation = useFeatureWallRailOrientation()
  const compactActiveStepRef = useRef<HTMLButtonElement | null>(null)
  const selectedWorkflow = workflows.find((workflow) => workflow.id === props.selectedWorkflowId)

  useEffect(() => {
    if (orientation !== 'horizontal') {
      return
    }
    const selectedWorkflowIndex = workflows.findIndex(
      (workflow) => workflow.id === props.selectedWorkflowId
    )
    props.railRefs.current[selectedWorkflowIndex]?.scrollIntoView({
      block: 'nearest',
      inline: 'nearest'
    })
    compactActiveStepRef.current?.scrollIntoView({ block: 'nearest', inline: 'nearest' })
  }, [orientation, props.railRefs, props.selectedStepId, props.selectedWorkflowId, workflows])

  useEffect(() => {
    if (typeof window.matchMedia !== 'function') {
      return
    }
    const media = window.matchMedia(DESKTOP_RAIL_MEDIA_QUERY)
    const handleBreakpointChange = (): void => {
      // Why: compact and desktop rails render separate step rows. Transfer
      // focus before the media query hides the currently focused copy.
      if (hasFocusedFeatureWallStep()) {
        focusVisibleFeatureWallStep(props.selectedStepId)
      }
    }
    media.addEventListener('change', handleBreakpointChange)
    return () => media.removeEventListener('change', handleBreakpointChange)
  }, [props.selectedStepId])

  return (
    <nav
      className="scrollbar-sleek overflow-hidden border-b border-border bg-card md:h-full md:overflow-y-auto md:border-b-0 md:p-2"
      aria-label={translate('auto.components.feature.wall.FeatureWallRail.7593d15f94', 'Workflows')}
    >
      <div
        role="tablist"
        aria-orientation={orientation}
        data-feature-wall-navigation-row="workflows"
        className="scrollbar-sleek flex gap-1 overflow-x-auto p-2 [@media(max-height:500px)]:p-1 md:flex-col md:gap-1.5 md:overflow-x-visible md:p-0 md:pt-1.5"
      >
        {workflows.map((workflow, workflowIndex) => {
          const isSelected = workflow.id === props.selectedWorkflowId
          const isDone = props.workflowDone[workflow.id]
          const workflowTabId = `${props.previewPanelId}-workflow-${workflow.id}`
          return (
            <div key={workflow.id} className="shrink-0 md:w-full">
              <button
                ref={(node) => {
                  props.railRefs.current[workflowIndex] = node
                }}
                id={workflowTabId}
                type="button"
                role="tab"
                aria-selected={isSelected}
                aria-controls={props.previewPanelId}
                tabIndex={isSelected ? 0 : -1}
                data-feature-wall-workflow-id={workflow.id}
                onClick={() => props.onSelectWorkflow(workflow)}
                onKeyDown={(event) => props.onRailKeyDown(event, workflowIndex, orientation)}
                className={cn(
                  'flex w-auto shrink-0 items-center gap-1.5 rounded-md px-2 py-1.5 text-left text-xs outline-none transition-colors motion-reduce:transition-none md:w-full md:gap-2.5 md:px-2.5 md:py-2 md:text-sm',
                  'hover:bg-accent focus-visible:ring-[3px] focus-visible:ring-ring/50',
                  isSelected && 'bg-accent text-accent-foreground'
                )}
              >
                <ProgressMarker
                  compact={orientation === 'horizontal'}
                  done={isDone}
                  label={String(workflowIndex + 1)}
                />
                <span className="min-w-0 truncate font-medium leading-tight">{workflow.title}</span>
              </button>

              <div
                aria-hidden={!isSelected}
                className={cn(
                  'hidden overflow-hidden transition-[grid-template-rows,opacity] duration-200 ease-out motion-reduce:transition-none md:grid',
                  isSelected ? 'grid-rows-[1fr] opacity-100' : 'grid-rows-[0fr] opacity-0'
                )}
              >
                <div className="min-h-0">
                  <FeatureWallStepButtons
                    workflow={workflow}
                    selectedStepId={props.selectedStepId}
                    interactive={isSelected}
                    stepDone={props.stepDone}
                    onSelectStep={props.onSelectStep}
                  />
                </div>
              </div>
            </div>
          )
        })}
      </div>

      {selectedWorkflow ? (
        <div
          role="group"
          aria-labelledby={`${props.previewPanelId}-workflow-${selectedWorkflow.id}`}
          data-feature-wall-navigation-row="steps"
          className="scrollbar-sleek overflow-x-auto border-t border-border p-2 [@media(max-height:500px)]:p-1 md:hidden"
        >
          <FeatureWallStepButtons
            compact
            workflow={selectedWorkflow}
            selectedStepId={props.selectedStepId}
            interactive
            stepDone={props.stepDone}
            activeStepRef={compactActiveStepRef}
            onSelectStep={props.onSelectStep}
          />
        </div>
      ) : null}
    </nav>
  )
}

function FeatureWallStepButtons(props: {
  compact?: boolean
  workflow: FeatureWallWorkflow
  selectedStepId: FeatureWallStepId
  interactive: boolean
  stepDone: Record<FeatureWallStepId, boolean>
  activeStepRef?: MutableRefObject<HTMLButtonElement | null>
  onSelectStep: (workflow: FeatureWallWorkflow, stepId: FeatureWallStepId) => void
}): JSX.Element {
  return (
    <div className={props.compact ? 'flex gap-1' : 'mt-1 flex flex-col gap-1 pl-7'}>
      {props.workflow.steps.map((step, stepIndex) => {
        const isStepActive = step.id === props.selectedStepId
        return (
          <button
            key={step.id}
            ref={(node) => {
              if (isStepActive && props.activeStepRef) {
                props.activeStepRef.current = node
              }
            }}
            type="button"
            data-feature-wall-step-id={step.id}
            tabIndex={props.interactive ? 0 : -1}
            onClick={() => props.onSelectStep(props.workflow, step.id)}
            aria-current={isStepActive ? 'step' : undefined}
            className={cn(
              'flex items-center gap-2 rounded-md text-left outline-none transition-colors motion-reduce:transition-none',
              props.compact
                ? 'w-auto shrink-0 px-2 py-1.5 text-xs'
                : 'w-full px-2.5 py-1.5 text-[13px]',
              'hover:bg-accent focus-visible:ring-[3px] focus-visible:ring-ring/50',
              isStepActive && 'bg-accent text-accent-foreground'
            )}
          >
            <ProgressMarker
              compact
              done={props.stepDone[step.id]}
              label={`${SUB_STEP_LABELS[stepIndex] ?? stepIndex + 1}.`}
            />
            <span
              className={cn(
                'truncate leading-tight',
                isStepActive ? 'font-medium' : 'text-muted-foreground'
              )}
            >
              {step.name}
            </span>
          </button>
        )
      })}
    </div>
  )
}

function ProgressMarker(props: { done: boolean; label: string; compact?: boolean }): JSX.Element {
  return (
    <span
      className={cn(
        'flex shrink-0 items-center justify-center rounded-sm border font-mono',
        props.compact ? 'size-5 text-[10px]' : 'size-7 text-xs',
        props.done
          ? 'border-border bg-muted text-foreground'
          : 'border-border bg-card text-muted-foreground'
      )}
      aria-label={
        props.done
          ? translate('auto.components.feature.wall.FeatureWallRail.69ea857689', 'Viewed')
          : undefined
      }
    >
      {props.done ? (
        <Check className={props.compact ? 'size-3' : 'size-3.5'} aria-hidden />
      ) : (
        props.label
      )}
    </span>
  )
}

function readFeatureWallRailOrientation(): FeatureWallRailOrientation {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') {
    return 'vertical'
  }
  return window.matchMedia(DESKTOP_RAIL_MEDIA_QUERY).matches ? 'vertical' : 'horizontal'
}

function subscribeFeatureWallRailOrientation(onChange: () => void): () => void {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') {
    return () => {}
  }
  // Why: ARIA orientation and arrow keys must follow the same breakpoint as
  // the CSS layout so compact navigation remains keyboard-correct after resize.
  const media = window.matchMedia(DESKTOP_RAIL_MEDIA_QUERY)
  media.addEventListener('change', onChange)
  return () => media.removeEventListener('change', onChange)
}

function useFeatureWallRailOrientation(): FeatureWallRailOrientation {
  return useSyncExternalStore(
    subscribeFeatureWallRailOrientation,
    readFeatureWallRailOrientation,
    () => 'vertical'
  )
}
