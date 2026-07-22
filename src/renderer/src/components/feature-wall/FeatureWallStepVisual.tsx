import type { JSX } from 'react'
import type { FeatureWallStepId } from '../../../../shared/feature-wall-workflows'
import { translate } from '@/i18n/i18n'
import { cn } from '@/lib/utils'
import { AgentsOrchestrationVisual } from './AgentsOrchestrationVisual'
import { AgentAttentionWorkflowVisual } from './AgentAttentionWorkflowVisual'
import { AutomationWorkflowVisual } from './ScaleWorkflowVisuals'
import { ComputerUseWorkflowVisual, RemoteMobileWorkflowVisual } from './AnywhereWorkflowVisuals'
import { BrowserAnimatedVisual } from './BrowserAnimatedVisual'
import { CliSkillsWorkflowVisual } from './CliSkillsWorkflowVisual'
import { MobileEmulatorsWorkflowVisual } from './MobileEmulatorsWorkflowVisual'
import { ReviewShipWorkflowVisual } from './ReviewShipWorkflowVisual'
import {
  AddProjectWorkflowVisual,
  TerminalFirstWorkflowVisual
} from './TerminalProjectWorkflowVisuals'
import { TasksAnimatedVisual } from './TasksAnimatedVisual'
import { WorkbenchContextWorkflowVisual } from './WorkbenchContextWorkflowVisual'
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
        {/* Why: every workflow is a storyboard, so representative data must never read as live. */}
        <div className="mb-1 flex justify-end">
          <span
            className="rounded-full border border-border bg-background px-2 py-0.5 text-[11px] font-medium text-muted-foreground"
            data-feature-wall-illustrative-example="true"
          >
            {translate(
              'auto.components.feature.wall.FeatureWallStepVisual.k130000001',
              'Illustrative example'
            )}
          </span>
        </div>
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
      return <AgentAttentionWorkflowVisual reducedMotion={reducedMotion} />
    case 'workbench':
      return <WorkbenchContextWorkflowVisual reducedMotion={reducedMotion} />
    case 'browser-design':
      return <BrowserAnimatedVisual reducedMotion={reducedMotion} />
    case 'review-ship':
      return <ReviewShipWorkflowVisual reducedMotion={reducedMotion} />
    case 'cli-skills':
      return <CliSkillsWorkflowVisual reducedMotion={reducedMotion} />
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
      return <AutomationWorkflowVisual reducedMotion={reducedMotion} />
    case 'remote-mobile':
      return <RemoteMobileWorkflowVisual />
    case 'mobile-emulators':
      return <MobileEmulatorsWorkflowVisual />
    case 'computer-use':
      return <ComputerUseWorkflowVisual />
  }
}

function visualWidth(stepId: FeatureWallStepId): string {
  switch (stepId) {
    case 'workspaces':
      return 'max-w-[440px]'
    case 'tasks':
    case 'orchestration':
      return 'max-w-[520px]'
    case 'workbench':
      return 'max-w-[560px]'
    case 'terminal':
    case 'add-project':
    case 'agents':
    case 'browser-design':
    case 'review-ship':
    case 'cli-skills':
    case 'automations':
    case 'remote-mobile':
    case 'mobile-emulators':
    case 'computer-use':
      return 'max-w-[660px]'
  }
}
