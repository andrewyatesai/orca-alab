import type { JSX } from 'react'
import type { FeatureWallStepId } from '../../../../shared/feature-wall-workflows'
import { cn } from '@/lib/utils'
import { AgentsOrchestrationVisual } from './AgentsOrchestrationVisual'
import { AutomationWorkflowVisual } from './ScaleWorkflowVisuals'
import { ComputerUseWorkflowVisual, RemoteMobileWorkflowVisual } from './AnywhereWorkflowVisuals'
import { ReviewShipWorkflowVisual } from './ReviewShipWorkflowVisual'
import {
  AddProjectWorkflowVisual,
  TerminalFirstWorkflowVisual
} from './TerminalProjectWorkflowVisuals'
import { TasksAnimatedVisual } from './TasksAnimatedVisual'
import { WorkbenchAnimatedVisual } from './WorkbenchAnimatedVisual'
import { WorkspacesAnimatedVisual } from './WorkspacesAnimatedVisual'

export function FeatureWallStepVisual(props: {
  stepId: FeatureWallStepId
  reducedMotion: boolean
}): JSX.Element {
  return (
    <div
      className="flex min-h-[420px] w-full items-center justify-center [@media(max-height:500px)]:items-start"
      data-feature-wall-step-visual={props.stepId}
      key={props.stepId}
      aria-hidden="true"
    >
      <div
        className={cn('w-full max-w-full', visualWidth(props.stepId))}
        data-feature-wall-visual-content
      >
        {renderVisual(props.stepId, props.reducedMotion)}
      </div>
    </div>
  )
}

function renderVisual(stepId: FeatureWallStepId, reducedMotion: boolean): JSX.Element {
  switch (stepId) {
    case 'terminal':
      return <TerminalFirstWorkflowVisual />
    case 'add-project':
      return <AddProjectWorkflowVisual />
    case 'tasks':
      return <TasksAnimatedVisual reducedMotion={reducedMotion} />
    case 'workspaces':
      return <WorkspacesAnimatedVisual reducedMotion={reducedMotion} />
    case 'agents':
      return (
        <AgentsOrchestrationVisual
          reducedMotion={reducedMotion}
          activeStepId="statuses"
          widthPx={520}
          heightPx={360}
        />
      )
    case 'workbench':
      return <WorkbenchAnimatedVisual reducedMotion={reducedMotion} />
    case 'review-ship':
      return <ReviewShipWorkflowVisual />
    case 'orchestration':
      return (
        <AgentsOrchestrationVisual
          reducedMotion={reducedMotion}
          activeStepId="orchestration"
          widthPx={520}
          heightPx={392}
        />
      )
    case 'automations':
      return <AutomationWorkflowVisual />
    case 'remote-mobile':
      return <RemoteMobileWorkflowVisual />
    case 'computer-use':
      return <ComputerUseWorkflowVisual />
  }
}

function visualWidth(stepId: FeatureWallStepId): string {
  switch (stepId) {
    case 'workspaces':
      return 'max-w-[440px]'
    case 'tasks':
    case 'agents':
    case 'orchestration':
      return 'max-w-[520px]'
    case 'workbench':
      return 'max-w-[560px]'
    case 'terminal':
    case 'add-project':
    case 'review-ship':
    case 'automations':
    case 'remote-mobile':
    case 'computer-use':
      return 'max-w-[660px]'
  }
}
