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
  | 'review-ship'
  | 'orchestration'
  | 'automations'
  | 'remote-mobile'
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
          'When a workspace is active, Orca opens its terminal by default. Run any CLI agent, split tabs and panes, and return to persistent sessions after reconnecting.',
        docsUrl: 'https://www.onorca.dev/docs/terminal'
      },
      {
        id: 'add-project',
        name: 'Add a project',
        title: 'Bring in a codebase',
        description:
          'Choose where project operations run—this computer, an SSH host, or a paired Orca runtime—then open an existing folder, clone a repository, or create a project. Existing checkouts stay on their current branch.',
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
        name: 'Workspaces',
        title: 'Keep every task isolated',
        description:
          'Each Git workspace gets its own worktree, branch, terminals, files, browser state, and agent sessions so parallel work does not collide.',
        docsUrl: 'https://www.onorca.dev/docs/model/worktrees'
      }
    ]
  },
  {
    id: 'build',
    title: 'Build',
    meta: 'Agents · Workbench',
    lede: 'Run the agents you already use and keep implementation context in one place.',
    primaryTileId: 'tile-04',
    relatedTileIds: ['tile-05', 'tile-07', 'tile-12'],
    docsUrl: 'https://www.onorca.dev/docs/agents/supported',
    steps: [
      {
        id: 'agents',
        name: 'Agent fleet',
        title: 'See every agent at a glance',
        description:
          'Run any terminal-based agent. For supported agents, see which sessions are working, waiting, blocked, or done without opening every terminal.',
        docsUrl: 'https://www.onorca.dev/docs/agents/supported'
      },
      {
        id: 'workbench',
        name: 'Workbench',
        title: 'Keep implementation context together',
        description:
          'Move between terminal splits, the editor, files, ports, and the embedded browser; send selected UI context to agents with Design Mode.',
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
        title: 'Review, revise, then publish',
        description:
          'Annotate local diffs and send one focused revision bundle to the agent. With a connected Git provider, inspect hosted checks and prepare a commit plus pull or merge request.',
        docsUrl: 'https://www.onorca.dev/docs/review/annotate-ai-diff'
      }
    ]
  },
  {
    id: 'scale',
    title: 'Scale',
    meta: 'Orchestration · Automations',
    lede: 'Coordinate dependent work and make repeatable jobs run on demand or on schedule.',
    primaryTileId: 'tile-09',
    relatedTileIds: ['tile-04', 'tile-11'],
    docsUrl: 'https://www.onorca.dev/docs/cli/orchestration',
    steps: [
      {
        id: 'orchestration',
        name: 'Orchestration',
        title: 'Coordinate work that needs a team',
        description:
          'Give a coordinator dependent tasks, let workers report progress or blockers, and collect their results into one accountable run.',
        docsUrl: 'https://www.onorca.dev/docs/cli/orchestration'
      },
      {
        id: 'automations',
        name: 'Automations',
        title: 'Make recurring work repeatable',
        description:
          'Save a prompt and target, then run it manually or on a schedule. Remote targets must be reachable; each run can use a fresh or existing workspace and stays visible in history.',
        docsUrl: 'https://www.onorca.dev/docs/cli/automations'
      }
    ]
  },
  {
    id: 'anywhere',
    title: 'Anywhere',
    meta: 'SSH · Mobile · Computer Use',
    lede: 'Connect the desktop and its mobile companion through an Orca runtime, reach separate project hosts over SSH, and, where supported, operate visible desktop apps.',
    primaryTileId: 'tile-06',
    relatedTileIds: [],
    docsUrl: 'https://www.onorca.dev/docs/ssh',
    steps: [
      {
        id: 'remote-mobile',
        name: 'Remote & mobile',
        title: 'Keep work moving away from this machine',
        description:
          'After one-time pairing, Orca Mobile acts as a companion for monitoring and follow-ups while the desktop/runtime remains the source of truth. Projects can run there or on a separate SSH host.',
        availabilityLabel: 'Mobile beta',
        docsUrl: 'https://www.onorca.dev/docs/mobile'
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
