import {
  FEATURE_WALL_WORKFLOWS,
  type FeatureWallWorkflow
} from '../../../../shared/feature-wall-workflows'
import { createLocalizedCatalog } from '@/i18n/localized-catalog'
import { getLocalizedFeatureWallStepCopy } from './feature-wall-step-localization'
import { getLocalizedFeatureWallWorkflowCopy } from './feature-wall-workflow-localization'

export const getLocalizedFeatureWallWorkflows = createLocalizedCatalog(
  (): readonly FeatureWallWorkflow[] =>
    FEATURE_WALL_WORKFLOWS.map((workflow) => ({
      ...workflow,
      ...getLocalizedFeatureWallWorkflowCopy(workflow.id),
      steps: workflow.steps.map((step) => ({
        ...step,
        ...getLocalizedFeatureWallStepCopy(step.id)
      }))
    }))
)
