import { translate } from '@/i18n/i18n'
import type { OrchestrationMessageId } from './orchestration-types'

export function getOrchestrationMessage(message: OrchestrationMessageId): string {
  switch (message) {
    case 'coord-splitting':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000001',
        'Splitting auth rewrite into 2 PRs…'
      )
    case 'child-codex-writing':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000002',
        'Writing the users table migration…'
      )
    case 'child-claude-sketching':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000003',
        'Sketching withSession middleware…'
      )
    case 'child-codex-adding-column':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000004',
        'Adding the email_verified column…'
      )
    case 'child-claude-wiring-middleware':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000005',
        'Wiring withSession middleware…'
      )
    case 'child-codex-ready':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000006',
        'PR 1/2 ready'
      )
    case 'child-claude-ready':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000007',
        'PR 2/2 ready'
      )
  }
}

export function getOrchestrationWorkspaceName(
  workspace: 'coordinator' | 'migration' | 'middleware'
): string {
  switch (workspace) {
    case 'coordinator':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000008',
        'redesign auth flow'
      )
    case 'migration':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000009',
        'PR 1/2: migrate users.sql'
      )
    case 'middleware':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000010',
        'PR 2/2: withSession middleware'
      )
  }
}
