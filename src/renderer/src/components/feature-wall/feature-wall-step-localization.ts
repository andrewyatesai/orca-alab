import type { FeatureWallStep, FeatureWallStepId } from '../../../../shared/feature-wall-workflows'
import { translate } from '@/i18n/i18n'

type FeatureWallStepCopy = Pick<
  FeatureWallStep,
  'name' | 'title' | 'description' | 'availabilityLabel'
>

export function getLocalizedFeatureWallStepCopy(id: FeatureWallStepId): FeatureWallStepCopy {
  switch (id) {
    case 'terminal':
      return {
        name: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000001',
          'Terminal first'
        ),
        title: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000002',
          'Start in a terminal—project or scratch'
        ),
        description: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g130000007',
          'Orca opens a workspace terminal by default—or a local scratch terminal before a project exists. Run any CLI agent, split tabs or panes, and review then launch Quick Commands. Project commands from orca.yaml stay inert until you approve the current shared content; changes require re-review. GPU/CPU rendering, focus-aware QoS, predictive echo, full-scrollback search, inline images, terminal effects, and bundled build tools support heavy agent work. Warm sessions reattach after an app restart; after a host reboot, Orca restores layout and scrollback, not exited processes.'
        )
      }
    case 'add-project':
      return {
        name: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000004',
          'Add a project'
        ),
        title: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000005',
          'Bring in a codebase'
        ),
        description: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g150000001',
          'Choose where project operations run—this computer (native or WSL), an SSH host, or a paired Orca runtime—then open an existing folder, clone a repository, or create a project. Existing checkouts stay on their current branch. When you later create a Git workspace, configured repository setup/install commands can run automatically in its new worktree after you approve shared orca.yaml command content; changes require re-review.'
        )
      }
    case 'tasks':
      return {
        name: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000007',
          'Tasks'
        ),
        title: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000008',
          'Turn tasks into ready-to-run work'
        ),
        description: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000009',
          'Connect the providers you use, browse GitHub, GitLab, Linear, and Jira work, then carry an issue or review into a workspace as linked context.'
        )
      }
    case 'workspaces':
      return {
        name: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g130000008',
          'Race approaches'
        ),
        title: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g130000009',
          'Plan, fan out, and keep the winner'
        ),
        description: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g140000001',
          'Use the Workspace Board to organize existing workspaces in status lanes. For a Git project, fan one task into isolated worktrees from the same base, compare each diff and check result, keep the winner, then archive alternatives. Folder-only projects keep sharing their original root; the board does not launch or merge the race.'
        )
      }
    case 'agents':
      return {
        name: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g130000011',
          'Agents & attention'
        ),
        title: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g130000012',
          'Intervene now, recover context later'
        ),
        description: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g130000013',
          'Run supported or custom terminal agents, then use statuses, the optional Agents feed, and notifications to reach waiting or blocked work. Search Agent Session History, inspect a log when available, jump to its worktree, or resume when the transcript has conversation content and the target workspace and host are compatible. Manual leaves agent permission checks enabled; full autonomy asks supported agents to bypass them. Neither makes worktrees a machine-security sandbox.'
        )
      }
    case 'workbench':
      return {
        name: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000016',
          'Workbench'
        ),
        title: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g130000014',
          'Move through code and context without friction'
        ),
        description: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g130000015',
          'Use Quick Open and the Jump Palette across workspace tabs, files, settings, actions, ports, and rich previews; drag context into an agent prompt. Use the default-on Floating Workspace for cross-repo or scratch terminal, agent, Markdown, and browser tabs. Its directory and tabs stay local while an SSH or paired-runtime workspace is focused. Optional Voice Dictation transcribes into the focused pane after model and microphone setup.'
        )
      }
    case 'browser-design':
      return {
        name: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g130000001',
          'Browser & Design Mode'
        ),
        title: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g130000002',
          'Turn rendered UI into precise agent context'
        ),
        description: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g130000003',
          'Open a real Chromium page for the workspace, select an element in Design Mode, and send its DOM and computed styles, plus a source hint and cropped screenshot when available, to an agent. Import cookies into a selected browser profile only when you choose to reuse an authenticated session. Review the context, hot-reload the result, and verify the changed state.'
        )
      }
    case 'review-ship':
      return {
        name: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000019',
          'Review & ship'
        ),
        title: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g130000016',
          'Compare, recover, and publish deliberately'
        ),
        description: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g140000002',
          'Compare candidate diffs, annotate lines, and send a revision bundle. If checks fail or conflicts surface, return to the same workspace, resolve, and retry; then have a human re-review the resolved diff and refreshed checks before staging focused hunks. Confirm Git writes and PR/MR publishing separately, then archive the finished workspace.'
        )
      }
    case 'cli-skills':
      return {
        name: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g130000004',
          'CLI & Skills'
        ),
        title: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g130000005',
          'Let agents drive Orca itself'
        ),
        description: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g130000006',
          'The Orca CLI and version-matched bundled, personal, repository, and plugin skills let agents operate workspaces, terminals, files, browsers, and automations. Discovery follows the host that runs the work—local, SSH, or paired runtime.'
        )
      }
    case 'orchestration':
      return {
        name: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000022',
          'Orchestration'
        ),
        title: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g130000018',
          'Coordinate work with dependencies'
        ),
        description: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g130000019',
          'Use a simple workspace race for independent approaches. When tasks depend on one another, give a coordinator a worker graph, pause for human decisions when workers raise questions, relay those decisions, recover blockers, and collect the results into one accountable run.'
        )
      }
    case 'automations':
      return {
        name: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000025',
          'Automations'
        ),
        title: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000026',
          'Make recurring work repeatable'
        ),
        description: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g130000020',
          'Save a prompt and target, add an optional precheck, then run manually or on a schedule in a fresh or existing workspace. Inspect history, recover failed runs, and rerun when the selected local or remote target is reachable.'
        )
      }
    case 'remote-mobile':
      return {
        name: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000028',
          'Remote & mobile'
        ),
        title: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000029',
          'Keep work moving away from this machine'
        ),
        description: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g130000021',
          'Run projects locally, over SSH, on a paired runtime, or in an on-demand environment described by orca.yaml. After one-time pairing, Orca Mobile remains a companion for notifications, monitoring, Quick Commands, and follow-ups; the desktop/runtime coordinates the session, while the selected local or SSH host retains execution authority.'
        ),
        availabilityLabel: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000035',
          'Mobile beta'
        )
      }
    case 'mobile-emulators':
      return {
        name: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g140000003',
          'App emulators'
        ),
        title: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g140000004',
          'Drive iOS and Android test devices'
        ),
        description: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g140000005',
          "On a Mac with Xcode, open Orca's workspace-scoped iOS Simulator pane. On macOS, Linux, or Windows, target a booted Android emulator or physical ADB device through the same CLI namespace and stream it into Orca's workspace Emulator pane; an AVD can also keep its own window open. Let an agent load the version-matched skill, discover the exact device, inspect accessibility or logs, act, and verify; iOS control is local to the Mac."
        )
      }
    case 'computer-use':
      return {
        name: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000031',
          'Computer Use'
        ),
        title: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000032',
          'Operate desktop apps with guardrails'
        ),
        description: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000033',
          "Computer Use ships native helpers per platform. On macOS, grant Accessibility and Screen Recording; on every platform, check capabilities before inspecting a visible app and invoking advertised actions. Use Orca's browser tools for pages inside Orca."
        ),
        availabilityLabel: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000034',
          'Beta'
        )
      }
  }
}
