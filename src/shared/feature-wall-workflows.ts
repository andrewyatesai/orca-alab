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
    lede: 'Resume in the active workspace terminal, then add another codebase and runtime when you need one.',
    primaryTileId: 'tile-02',
    relatedTileIds: ['tile-09'],
    docsUrl: 'https://www.onorca.dev/docs/terminal',
    steps: [
      {
        id: 'terminal',
        name: 'Terminal first',
        title: 'Resume in the active workspace terminal',
        description:
          'When a workspace is active, Orca opens its terminal by default. Run any CLI agent and split tabs or panes. Warm sessions reattach after an app restart; after a host reboot, Orca restores layout and scrollback rather than exited processes.',
        docsUrl: 'https://www.onorca.dev/docs/terminal'
      },
      {
        id: 'add-project',
        name: 'Add a project',
        title: 'Bring in a codebase',
        description:
          'Choose where project operations run—this computer (native or WSL), an SSH host, or a paired Orca runtime—then open an existing folder, clone a repository, or create a project. Existing checkouts stay on their current branch.',
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
        title: 'Fan out, compare, and keep the winner',
        description:
          'For a Git project, create several isolated worktrees from the same base, give agents the same task, compare their diffs and checks, keep the best result, then archive the alternatives. Folder-only projects continue to share their original root.',
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
        title: 'Know where to intervene',
        description:
          'Run supported or custom terminal agents, then use statuses, the Agents feed, and notifications to jump to work that is waiting or blocked. Manual mode asks before sensitive actions; full autonomy uses host access, and worktrees are not a machine-security sandbox.',
        docsUrl: 'https://www.onorca.dev/docs/agents/supported'
      },
      {
        id: 'workbench',
        name: 'Workbench',
        title: 'Move through code and context without friction',
        description:
          'Use Quick Open and the Jump Palette across terminals, editors, files, ports, and rich Markdown, PDF, image, CSV, and notebook previews; drag files or images directly into an agent prompt.',
        docsUrl: 'https://www.onorca.dev/docs/model/quick-open'
      },
      {
        id: 'browser-design',
        name: 'Browser & Design Mode',
        title: 'Turn rendered UI into precise agent context',
        description:
          'Open a real Chromium page for the workspace, select an element in Design Mode, and send its DOM, CSS, source, and cropped screenshot to an agent. Hot-reload the result, exercise it, and verify the changed state.',
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
          'Compare candidate diffs, annotate lines, send a revision bundle, and stage focused hunks. If checks fail or conflicts surface, return to the same workspace, resolve, and retry. Confirm Git writes and PR/MR publishing separately, then archive the finished workspace.',
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
          'Use a simple workspace race for independent approaches. When tasks depend on one another, give a coordinator a worker graph, surface questions and blockers, and collect decisions and results into one accountable run.',
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
          'Run projects locally, over SSH, on a paired runtime, or in an on-demand environment described by orca.yaml. After one-time pairing, Orca Mobile remains a companion for notifications, monitoring, Quick Commands, and follow-ups; the desktop/runtime remains the source of truth and execution authority.',
        availabilityLabel: 'Mobile beta',
        docsUrl: 'https://www.onorca.dev/docs/mobile'
      },
      {
        id: 'mobile-emulators',
        name: 'App emulators',
        title: 'Drive iOS and Android test devices',
        description:
          "On a Mac with Xcode, open Orca's workspace-scoped iOS Simulator pane. On macOS, Linux, or Windows, target a booted Android emulator or ADB device through the same CLI namespace and watch it in its emulator window. Let an agent load the version-matched skill, discover the exact device, inspect accessibility or logs, act, and verify; iOS control is local to the Mac.",
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
