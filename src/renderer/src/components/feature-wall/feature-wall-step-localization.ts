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
          'Resume in the active workspace terminal'
        ),
        description: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000003',
          'When a workspace is active, Orca opens its terminal by default. Run any CLI agent, split tabs and panes, and return to persistent sessions after reconnecting.'
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
          'auto.components.feature.wall.feature-wall-step-localization.g120000006',
          'Choose where project operations run—this computer, an SSH host, or a paired Orca runtime—then open an existing folder, clone a repository, or create a project. Existing checkouts stay on their current branch.'
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
          'auto.components.feature.wall.feature-wall-step-localization.g120000010',
          'Workspaces'
        ),
        title: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000011',
          'Keep every task isolated'
        ),
        description: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000012',
          'Each Git workspace gets its own worktree, branch, terminals, files, browser state, and agent sessions so parallel work does not collide.'
        )
      }
    case 'agents':
      return {
        name: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000013',
          'Agent fleet'
        ),
        title: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000014',
          'See every agent at a glance'
        ),
        description: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000015',
          'Run any terminal-based agent. For supported agents, see which sessions are working, waiting, blocked, or done without opening every terminal.'
        )
      }
    case 'workbench':
      return {
        name: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000016',
          'Workbench'
        ),
        title: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000017',
          'Keep implementation context together'
        ),
        description: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000018',
          'Move between terminal splits, the editor, files, ports, and the embedded browser; send selected UI context to agents with Design Mode.'
        )
      }
    case 'review-ship':
      return {
        name: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000019',
          'Review & ship'
        ),
        title: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000020',
          'Review, revise, then publish'
        ),
        description: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000021',
          'Annotate local diffs and send one focused revision bundle to the agent. With a connected Git provider, inspect hosted checks and prepare a commit plus pull or merge request.'
        )
      }
    case 'orchestration':
      return {
        name: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000022',
          'Orchestration'
        ),
        title: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000023',
          'Coordinate work that needs a team'
        ),
        description: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000024',
          'Give a coordinator dependent tasks, let workers report progress or blockers, and collect their results into one accountable run.'
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
          'auto.components.feature.wall.feature-wall-step-localization.g120000027',
          'Save a prompt and target, then run it manually or on a schedule. Remote targets must be reachable; each run can use a fresh or existing workspace and stays visible in history.'
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
          'auto.components.feature.wall.feature-wall-step-localization.g120000030',
          'After one-time pairing, Orca Mobile acts as a companion for monitoring and follow-ups while the desktop/runtime remains the source of truth. Projects can run there or on a separate SSH host.'
        ),
        availabilityLabel: translate(
          'auto.components.feature.wall.feature-wall-step-localization.g120000035',
          'Mobile beta'
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
