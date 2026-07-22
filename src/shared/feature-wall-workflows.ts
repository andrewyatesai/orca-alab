import {
  FEATURE_WALL_TILES,
  isFeatureWallMediaTile,
  type FeatureWallMediaTile,
  type FeatureWallMediaTileId
} from './feature-wall-tiles'

export type FeatureWallWorkflowId = 'start' | 'plan' | 'build' | 'ship' | 'scale' | 'anywhere'

export type FeatureWallStepId =
  | 'terminal'
  | 'add-project'
  | 'tasks'
  | 'workspaces'
  | 'agents'
  | 'workbench'
  | 'browser-design'
  | 'review-ship'
  | 'cli-skills'
  | 'orchestration'
  | 'automations'
  | 'remote-mobile'
  | 'mobile-emulators'
  | 'computer-use'

export type FeatureWallStep = {
  readonly id: FeatureWallStepId
  readonly name: string
  readonly title: string
  readonly description: string
  readonly availabilityLabel?: string
  readonly docsUrl?: string
}

export type FeatureWallWorkflow = {
  readonly id: FeatureWallWorkflowId
  readonly title: string
  readonly meta: string
  readonly lede: string
  readonly steps: readonly FeatureWallStep[]
  readonly primaryTileId: FeatureWallMediaTileId
  readonly relatedTileIds: readonly FeatureWallMediaTileId[]
  readonly docsUrl: string
}

export const FEATURE_WALL_WORKFLOWS: readonly FeatureWallWorkflow[] = [
  {
    id: 'start',
    title: 'Start',
    meta: 'Terminal · Projects',
    lede: 'Start in a local scratch terminal or resume the active workspace terminal, then add another codebase and runtime when you need one.',
    primaryTileId: 'tile-02',
    relatedTileIds: ['tile-09'],
    docsUrl: 'https://www.onorca.dev/docs/terminal',
    steps: [
      {
        id: 'terminal',
        name: 'Terminal first',
        title: 'Start in a terminal—project or scratch',
        description:
          'Orca opens a workspace terminal by default—or a local scratch terminal before a project exists. Run any CLI agent, split tabs or panes, and review then launch Quick Commands. Project commands from orca.yaml stay inert until you approve the current shared content; changes require re-review. GPU/CPU rendering, focus-aware QoS, predictive echo, full-scrollback search, inline images, terminal effects, and bundled build tools support heavy agent work. Warm sessions reattach after an app restart; after a host reboot, Orca restores layout and scrollback, not exited processes.',
        docsUrl: 'https://www.onorca.dev/docs/terminal'
      },
      {
        id: 'add-project',
        name: 'Add a project',
        title: 'Bring in a codebase',
        description:
          'Choose where project operations run—this computer (native or WSL), an SSH host, or a paired Orca runtime—then open an existing folder, clone a repository, or create a project. Existing checkouts stay on their current branch. When you later create a Git workspace, configured repository setup/install commands can run automatically in its new worktree after you approve shared orca.yaml command content; changes require re-review.',
        docsUrl: 'https://www.onorca.dev/docs/model/worktrees'
      }
    ]
  },
  {
    id: 'plan',
    title: 'Plan',
    meta: 'Tasks · Workspaces',
    lede: 'Turn incoming work into an isolated environment with its context attached.',
    primaryTileId: 'tile-03',
    relatedTileIds: ['tile-01', 'tile-10'],
    docsUrl: 'https://www.onorca.dev/docs/model/worktrees',
    steps: [
      {
        id: 'tasks',
        name: 'Tasks',
        title: 'Turn tasks into ready-to-run work',
        description:
          'Connect the providers you use, browse GitHub, GitLab, Linear, and Jira work, then carry an issue or review into a workspace as linked context.',
        docsUrl: 'https://www.onorca.dev/docs/review/github'
      },
      {
        id: 'workspaces',
        name: 'Race approaches',
        title: 'Plan, fan out, and keep the winner',
        description:
          'Use the Workspace Board to organize existing workspaces in status lanes. For a Git project, fan one task into isolated worktrees from the same base, compare each diff and check result, keep the winner, then archive alternatives. Folder-only projects keep sharing their original root; the board does not launch or merge the race.',
        docsUrl: 'https://www.onorca.dev/docs/model/worktrees'
      }
    ]
  },
  {
    id: 'build',
    title: 'Build',
    meta: 'Agents · Workbench · Browser',
    lede: 'Guide the agents you already use, keep implementation context together, and verify UI work in place.',
    primaryTileId: 'tile-04',
    relatedTileIds: ['tile-11', 'tile-07', 'tile-12', 'tile-05'],
    docsUrl: 'https://www.onorca.dev/docs/agents/supported',
    steps: [
      {
        id: 'agents',
        name: 'Agents & attention',
        title: 'Intervene now, recover context later',
        description:
          'Run supported or custom terminal agents, then use statuses, the optional Agents feed, and notifications to reach waiting or blocked work. Search Agent Session History, inspect a log when available, jump to its worktree, or resume when the transcript has conversation content and the target workspace and host are compatible. Manual leaves agent permission checks enabled; full autonomy asks supported agents to bypass them. Neither makes worktrees a machine-security sandbox.',
        docsUrl: 'https://www.onorca.dev/docs/agents/supported'
      },
      {
        id: 'workbench',
        name: 'Workbench',
        title: 'Move through code and context without friction',
        description:
          'Use Quick Open and the Jump Palette across workspace tabs, files, settings, actions, ports, and rich previews; drag context into an agent prompt. Use the default-on Floating Workspace for cross-repo or scratch terminal, agent, Markdown, and browser tabs. Its directory and tabs stay local while an SSH or paired-runtime workspace is focused. Optional Voice Dictation transcribes into the focused pane after model and microphone setup.',
        docsUrl: 'https://www.onorca.dev/docs/model/quick-open'
      },
      {
        id: 'browser-design',
        name: 'Browser & Design Mode',
        title: 'Turn rendered UI into precise agent context',
        description:
          'Open a real Chromium page for the workspace, select an element in Design Mode, and send its DOM and computed styles, plus a source hint and cropped screenshot when available, to an agent. Import cookies into a selected browser profile only when you choose to reuse an authenticated session. Review the context, hot-reload the result, and verify the changed state.',
        docsUrl: 'https://www.onorca.dev/docs/browser/design-mode'
      }
    ]
  },
  {
    id: 'ship',
    title: 'Ship',
    meta: 'Review · Checks · Publish',
    lede: 'Turn an agent result into a reviewed, provider-ready change.',
    primaryTileId: 'tile-08',
    relatedTileIds: [],
    docsUrl: 'https://www.onorca.dev/docs/review/annotate-ai-diff',
    steps: [
      {
        id: 'review-ship',
        name: 'Review & ship',
        title: 'Compare, recover, and publish deliberately',
        description:
          'Compare candidate diffs, annotate lines, and send a revision bundle. If checks fail or conflicts surface, return to the same workspace, resolve, and retry; then have a human re-review the resolved diff and refreshed checks before staging focused hunks. Confirm Git writes and PR/MR publishing separately, then archive the finished workspace.',
        docsUrl: 'https://www.onorca.dev/docs/review/annotate-ai-diff'
      }
    ]
  },
  {
    id: 'scale',
    title: 'Scale',
    meta: 'CLI & Skills · Orchestration · Automations',
    lede: 'Let agents operate Orca, coordinate dependent work, and make repeatable jobs run on demand or on schedule.',
    primaryTileId: 'tile-09',
    relatedTileIds: ['tile-04', 'tile-11'],
    docsUrl: 'https://www.onorca.dev/docs/cli/orchestration',
    steps: [
      {
        id: 'cli-skills',
        name: 'CLI & Skills',
        title: 'Let agents drive Orca itself',
        description:
          'The Orca CLI and version-matched bundled, personal, repository, and plugin skills let agents operate workspaces, terminals, files, browsers, and automations. Discovery follows the host that runs the work—local, SSH, or paired runtime.',
        docsUrl: 'https://www.onorca.dev/docs/cli/skills'
      },
      {
        id: 'orchestration',
        name: 'Orchestration',
        title: 'Coordinate work with dependencies',
        description:
          'Use a simple workspace race for independent approaches. When tasks depend on one another, give a coordinator a worker graph, pause for human decisions when workers raise questions, relay those decisions, recover blockers, and collect the results into one accountable run.',
        docsUrl: 'https://www.onorca.dev/docs/cli/orchestration'
      },
      {
        id: 'automations',
        name: 'Automations',
        title: 'Make recurring work repeatable',
        description:
          'Save a prompt and target, add an optional precheck, then run manually or on a schedule in a fresh or existing workspace. Inspect history, recover failed runs, and rerun when the selected local or remote target is reachable.',
        docsUrl: 'https://www.onorca.dev/docs/cli/automations'
      }
    ]
  },
  {
    id: 'anywhere',
    title: 'Anywhere',
    meta: 'SSH · Mobile · Emulators · Computer Use',
    lede: 'Reach remote work, keep an eye on it from Mobile, exercise apps on iOS or Android, and, where supported, operate visible desktop software.',
    primaryTileId: 'tile-06',
    relatedTileIds: [],
    docsUrl: 'https://www.onorca.dev/docs/ssh',
    steps: [
      {
        id: 'remote-mobile',
        name: 'Remote & mobile',
        title: 'Keep work moving away from this machine',
        description:
          'Run projects locally, over SSH, on a paired runtime, or in an on-demand environment described by orca.yaml. After one-time pairing, Orca Mobile remains a companion for notifications, monitoring, Quick Commands, and follow-ups; the desktop/runtime coordinates the session, while the selected local or SSH host retains execution authority.',
        availabilityLabel: 'Mobile beta',
        docsUrl: 'https://www.onorca.dev/docs/mobile'
      },
      {
        id: 'mobile-emulators',
        name: 'App emulators',
        title: 'Drive iOS and Android test devices',
        description:
          "On a Mac with Xcode, open Orca's workspace-scoped iOS Simulator pane. On macOS, Linux, or Windows, target a booted Android emulator or physical ADB device through the same CLI namespace and stream it into Orca's workspace Emulator pane; an AVD can also keep its own window open. Let an agent load the version-matched skill, discover the exact device, inspect accessibility or logs, act, and verify; iOS control is local to the Mac.",
        docsUrl: 'https://www.onorca.dev/docs/cli/skills'
      },
      {
        id: 'computer-use',
        name: 'Computer Use',
        title: 'Operate desktop apps with guardrails',
        description:
          "Computer Use ships native helpers per platform. On macOS, grant Accessibility and Screen Recording; on every platform, check capabilities before inspecting a visible app and invoking advertised actions. Use Orca's browser tools for pages inside Orca.",
        availabilityLabel: 'Beta',
        docsUrl: 'https://www.onorca.dev/docs/cli/computer-use'
      }
    ]
  }
] as const

export const FEATURE_WALL_WORKFLOW_IDS = FEATURE_WALL_WORKFLOWS.map(
  (workflow) => workflow.id
) as readonly FeatureWallWorkflowId[]

export const FEATURE_WALL_STEP_IDS = FEATURE_WALL_WORKFLOWS.flatMap((workflow) =>
  workflow.steps.map((step) => step.id)
) as readonly FeatureWallStepId[]

const TILE_BY_ID = new Map(
  FEATURE_WALL_TILES.filter(isFeatureWallMediaTile).map((tile) => [tile.id, tile])
)

export function getFeatureWallMediaTile(id: FeatureWallMediaTileId): FeatureWallMediaTile | null {
  return TILE_BY_ID.get(id) ?? null
}

export const DEFAULT_FEATURE_WALL_WORKFLOW_ID: FeatureWallWorkflowId = 'start'
export const DEFAULT_FEATURE_WALL_STEP_ID: FeatureWallStepId = 'terminal'
