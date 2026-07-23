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
        'Illustrative example: Orca opens the active workspace terminal, or a local scratch terminal before any project exists. Review and run a project Quick Command, then see GPU/CPU rendering, focus-aware QoS, predictive echo, full-scrollback search, inline images, effects, and bundled build tools. A live session reattaches after a warm restart; a host reboot restores layout and scrollback.'
      )
    case 'add-project':
      return translate(
        'auto.components.feature.wall.FeatureWallStepVisual.k150000015',
        'Illustrative example: choose this computer or WSL, SSH, or a paired runtime and open an existing folder without changing its branch. After the project is added, explicitly create a Git workspace; Orca creates its isolated worktree, runs the approved shared orca.yaml setup command there, and leaves the workspace terminal and completed setup output visible.'
      )
    case 'tasks':
      return translate(
        'auto.components.feature.wall.FeatureWallStepVisual.k150000003',
        'Illustrative example: connect a task provider, open an issue as workspace context, wait while it is read, then open the ready workspace.'
      )
    case 'workspaces':
      return translate(
        'auto.components.feature.wall.FeatureWallStepVisual.k150000004',
        'Illustrative example: move an existing workspace card and see lane counts update, fan one Git task into isolated worktrees, compare checks and diffs, explicitly select the winner, and archive alternatives.'
      )
    case 'agents':
      return translate(
        'auto.components.feature.wall.FeatureWallStepVisual.k150000005',
        'Illustrative example: monitor agent attention, reply and recover sessions, switch accounts, then search Agent Session History; resume only with conversation content and a compatible target, jump to an owned worktree, or inspect an available local log.'
      )
    case 'workbench':
      return translate(
        'auto.components.feature.wall.FeatureWallStepVisual.k150000006',
        'Illustrative example: use Quick Open and the Jump Palette, make and autosave an edit, attach context, preview rich files, and dictate into the focused pane after model and microphone setup. The default-on local Floating Workspace holds cross-repository agents, scratch terminals, notes, and browser tabs while remote work stays remote.'
      )
    case 'browser-design':
      return translate(
        'auto.components.feature.wall.FeatureWallStepVisual.k150000007',
        'Illustrative example: select rendered UI, review the DOM, computed styles, optional source hint and cropped screenshot, destination, and sensitive context, send it to the workspace agent, then verify the updated page.'
      )
    case 'review-ship':
      return translate(
        'auto.components.feature.wall.FeatureWallStepVisual.k150000008',
        'Illustrative example: compare candidates, annotate a revision, and make a human review decision. Resolve failed checks or conflicts in the same workspace, rerun checks, then have a human re-review the resolved diff and refreshed checks before staging. Confirm Git and review-request writes separately, then archive.'
      )
    case 'cli-skills':
      return translate(
        'auto.components.feature.wall.FeatureWallStepVisual.k150000009',
        'Illustrative example: on an SSH host, discover a version-matched skill, create a worktree through the orca relay, inspect snapshots, act on an element reference, then verify with a fresh snapshot.'
      ).replaceAll('orca-dev', 'orca')
    case 'orchestration':
      return translate(
        'auto.components.feature.wall.FeatureWallStepVisual.k150000010',
        'Illustrative example: coordinate dependent tasks, pause when a worker asks a question, have a human resolve the decision gate, let the coordinator relay that approved decision, recover a failed contract check, and accept the accountable result.'
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
        'Illustrative example: select an exact iOS or Android device, inspect accessibility and logs, recover a missing or stale target by retrying emulator-5554, tap Email, type agent@example.com, and verify the updated profile state.'
      )
    case 'computer-use':
      return translate(
        'auto.components.feature.wall.FeatureWallStepVisual.k150000014',
        'Illustrative example: check platform capabilities and permissions, limit work to visible apps, inspect Terminal accessibility, invoke the advertised Reconnect action, and see the visible result “Agent connected.”'
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
