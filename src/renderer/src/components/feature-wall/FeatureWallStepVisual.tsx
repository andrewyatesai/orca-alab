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
      className="flex min-h-[420px] w-full items-center justify-center motion-reduce:[&_*]:animate-none! motion-reduce:[&_*]:transition-none! [@media(max-height:500px)]:items-start"
      data-feature-wall-step-visual={props.stepId}
      data-feature-wall-accessible-summary="true"
      key={props.stepId}
      role="img"
      aria-label={getAccessibleVisualSummary(props.stepId)}
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

function getAccessibleVisualSummary(stepId: FeatureWallStepId): string {
  switch (stepId) {
    case 'terminal':
      return translate(
        'auto.components.feature.wall.FeatureWallStepVisual.k150000001',
        'Illustrative example: a workspace opens in Terminal; a live session reattaches after a warm restart, while a host reboot restores layout and scrollback.'
      )
    case 'add-project':
      return translate(
        'auto.components.feature.wall.FeatureWallStepVisual.k150000002',
        'Illustrative example: choose this computer or WSL, SSH, or a paired runtime, then open, clone, or create the codebase without changing the current checkout.'
      )
    case 'tasks':
      return translate(
        'auto.components.feature.wall.FeatureWallStepVisual.k150000003',
        'Illustrative example: connect a task provider, open an issue as workspace context, wait while it is read, then open the ready workspace.'
      )
    case 'workspaces':
      return translate(
        'auto.components.feature.wall.FeatureWallStepVisual.k150000004',
        'Illustrative example: plan existing workspaces in status lanes, fan one Git task into isolated worktrees, compare checks and diffs, keep the winner, and archive alternatives.'
      )
    case 'agents':
      return translate(
        'auto.components.feature.wall.FeatureWallStepVisual.k150000005',
        'Illustrative example: monitor agent attention, reply and recover sessions, switch accounts, then search AI Vault; resume only with conversation content and a compatible target, jump to an owned worktree, or inspect an available local log.'
      )
    case 'workbench':
      return translate(
        'auto.components.feature.wall.FeatureWallStepVisual.k150000006',
        'Illustrative example: use Quick Open to edit and attach context, preview rich files, and use the default-on local Floating Workspace for cross-repository agents, scratch terminals, notes, and browser tabs.'
      )
    case 'browser-design':
      return translate(
        'auto.components.feature.wall.FeatureWallStepVisual.k150000007',
        'Illustrative example: select rendered UI, review the DOM, computed styles, optional source hint and cropped screenshot, destination, and sensitive context, send it to the workspace agent, then verify the updated page.'
      )
    case 'review-ship':
      return translate(
        'auto.components.feature.wall.FeatureWallStepVisual.k150000008',
        'Illustrative example: compare candidates, annotate a revision, make a human review decision, resolve failed checks or conflicts in the same workspace, confirm Git and review-request writes, then archive.'
      )
    case 'cli-skills':
      return translate(
        'auto.components.feature.wall.FeatureWallStepVisual.k150000009',
        'Illustrative example: discover a version-matched skill, create a worktree through orca-dev, inspect snapshots, act on an element reference, then verify with a fresh snapshot.'
      )
    case 'orchestration':
      return translate(
        'auto.components.feature.wall.FeatureWallStepVisual.k150000010',
        'Illustrative example: coordinate dependent tasks, answer a worker question, unblock a decision gate, recover a failed contract check, and accept the accountable result.'
      )
    case 'automations':
      return translate(
        'auto.components.feature.wall.FeatureWallStepVisual.k150000011',
        'Illustrative example: save scheduled or manual work with a precheck, preserve failed history, recover on rerun, and inspect the completed result.'
      )
    case 'remote-mobile':
      return translate(
        'auto.components.feature.wall.FeatureWallStepVisual.k150000012',
        'Illustrative example: keep SSH execution owned by the remote host, restore forwarded ports after reconnect, provision an orca.yaml environment, and monitor or reply from Mobile.'
      )
    case 'mobile-emulators':
      return translate(
        'auto.components.feature.wall.FeatureWallStepVisual.k150000013',
        'Illustrative example: select an exact iOS or Android device, inspect accessibility and logs, act and verify, and recover a missing or stale target before retrying.'
      )
    case 'computer-use':
      return translate(
        'auto.components.feature.wall.FeatureWallStepVisual.k150000014',
        'Illustrative example: check platform capabilities and permissions, limit work to visible apps, inspect accessibility, and invoke only actions the selected app advertises.'
      )
  }
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
