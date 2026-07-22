import type {
  FeatureWallWorkflow,
  FeatureWallWorkflowId
} from '../../../../shared/feature-wall-workflows'
import { translate } from '@/i18n/i18n'

type FeatureWallWorkflowCopy = Pick<FeatureWallWorkflow, 'title' | 'meta' | 'lede'>

export function getLocalizedFeatureWallWorkflowCopy(
  id: FeatureWallWorkflowId
): FeatureWallWorkflowCopy {
  switch (id) {
    case 'start':
      return {
        title: translate(
          'auto.components.feature.wall.feature-wall-workflow-localization.f120000001',
          'Start'
        ),
        meta: translate(
          'auto.components.feature.wall.feature-wall-workflow-localization.f120000002',
          'Terminal · Projects'
        ),
        lede: translate(
          'auto.components.feature.wall.feature-wall-workflow-localization.f120000003',
          'Resume in the active workspace terminal, then add another codebase and runtime when you need one.'
        )
      }
    case 'plan':
      return {
        title: translate(
          'auto.components.feature.wall.feature-wall-workflow-localization.f120000004',
          'Plan'
        ),
        meta: translate(
          'auto.components.feature.wall.feature-wall-workflow-localization.f120000005',
          'Tasks · Workspaces'
        ),
        lede: translate(
          'auto.components.feature.wall.feature-wall-workflow-localization.f120000006',
          'Turn incoming work into an isolated environment with its context attached.'
        )
      }
    case 'build':
      return {
        title: translate(
          'auto.components.feature.wall.feature-wall-workflow-localization.f120000007',
          'Build'
        ),
        meta: translate(
          'auto.components.feature.wall.feature-wall-workflow-localization.f120000008',
          'Agents · Workbench'
        ),
        lede: translate(
          'auto.components.feature.wall.feature-wall-workflow-localization.f120000009',
          'Run the agents you already use and keep implementation context in one place.'
        )
      }
    case 'ship':
      return {
        title: translate(
          'auto.components.feature.wall.feature-wall-workflow-localization.f120000010',
          'Ship'
        ),
        meta: translate(
          'auto.components.feature.wall.feature-wall-workflow-localization.f120000011',
          'Review · Checks · Publish'
        ),
        lede: translate(
          'auto.components.feature.wall.feature-wall-workflow-localization.f120000012',
          'Turn an agent result into a reviewed, provider-ready change.'
        )
      }
    case 'scale':
      return {
        title: translate(
          'auto.components.feature.wall.feature-wall-workflow-localization.f120000013',
          'Scale'
        ),
        meta: translate(
          'auto.components.feature.wall.feature-wall-workflow-localization.f120000014',
          'Orchestration · Automations'
        ),
        lede: translate(
          'auto.components.feature.wall.feature-wall-workflow-localization.f120000015',
          'Coordinate dependent work and make repeatable jobs run on demand or on schedule.'
        )
      }
    case 'anywhere':
      return {
        title: translate(
          'auto.components.feature.wall.feature-wall-workflow-localization.f120000016',
          'Anywhere'
        ),
        meta: translate(
          'auto.components.feature.wall.feature-wall-workflow-localization.f120000017',
          'SSH · Mobile · Computer Use'
        ),
        lede: translate(
          'auto.components.feature.wall.feature-wall-workflow-localization.f120000018',
          'Connect the desktop and its mobile companion through an Orca runtime, reach separate project hosts over SSH, and, where supported, operate visible desktop apps.'
        )
      }
  }
}
