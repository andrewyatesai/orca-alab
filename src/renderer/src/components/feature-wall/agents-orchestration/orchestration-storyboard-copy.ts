import { translate } from '@/i18n/i18n'
import type { OrchestrationMessageId, OrchestrationPhase } from './orchestration-types'

export function getOrchestrationMessage(message: OrchestrationMessageId): string {
  switch (message) {
    case 'coord-planning':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000053',
        'Mapping 2 tasks and their dependency…'
      )
    case 'migration-waiting-dispatch':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000054',
        'Waiting for migration dispatch…'
      )
    case 'middleware-waiting-dependency':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000055',
        'Waiting on Task 1 schema contract'
      )
    case 'coord-dispatching-migration':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000056',
        'Dispatching migration worker…'
      )
    case 'migration-running':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000057',
        'Adding email_verified + backfill…'
      )
    case 'coord-linking-dependency':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000058',
        'Holding middleware behind Task 1…'
      )
    case 'migration-blocking-question':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000059',
        'Question · Default legacy rows to false?'
      )
    case 'coord-decision-gate':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000011',
        'Decision gate · answer required'
      )
    case 'coord-human-decision-resolved':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h150000001',
        'You resolved the gate · false + explicit backfill'
      )
    case 'coord-decision-recorded':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000012',
        'Relaying your decision · false + explicit backfill'
      )
    case 'migration-applying-decision':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000013',
        'Human decision received · applying backfill…'
      )
    case 'migration-complete':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000014',
        'Task 1 complete · schema contract ready'
      )
    case 'coord-releasing-dependency':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000015',
        'Dependency satisfied · releasing Task 2'
      )
    case 'middleware-dependency-released':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000016',
        'Schema ready · wiring middleware…'
      )
    case 'middleware-check-blocked':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000017',
        'Blocked · auth contract check failed'
      )
    case 'coord-recovery-gate':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000018',
        'Blocker owned · recovery required'
      )
    case 'coord-recovery-plan':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000019',
        'Recovery · rebase, then rerun the check'
      )
    case 'middleware-rerunning-check':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000020',
        'Rebased · rerunning contract check…'
      )
    case 'middleware-recovered':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000021',
        'Recovered · contract checks pass'
      )
    case 'coord-result-complete':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000022',
        'Coordinator accepted the recovered run'
      )
  }
}

export function getOrchestrationWorkspaceName(
  workspace: 'coordinator' | 'migration' | 'middleware'
): string {
  switch (workspace) {
    case 'coordinator':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000060',
        'Coordinator · auth rewrite'
      )
    case 'migration':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000061',
        'Task 1 · migrate users.sql'
      )
    case 'middleware':
      return translate(
        'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000062',
        'Task 2 · withSession (after Task 1)'
      )
  }
}

export type OrchestrationPhaseCopy = {
  label: string
  headline: string
}

export function getOrchestrationPhaseCopy(phase: OrchestrationPhase): OrchestrationPhaseCopy {
  switch (phase) {
    case 'plan':
      return {
        label: translate(
          'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000023',
          'Plan'
        ),
        headline: translate(
          'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000024',
          'Coordinator maps the dependency graph'
        )
      }
    case 'dispatch':
      return {
        label: translate(
          'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000025',
          'Dispatch'
        ),
        headline: translate(
          'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000026',
          'Task 1 starts; Task 2 stays gated'
        )
      }
    case 'dependency':
      return {
        label: translate(
          'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000027',
          'Dependency'
        ),
        headline: translate(
          'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000028',
          'Middleware waits for the schema contract'
        )
      }
    case 'question':
      return {
        label: translate(
          'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000029',
          'Question'
        ),
        headline: translate(
          'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000030',
          'A worker pauses the graph for an answer'
        )
      }
    case 'decision':
      return {
        label: translate(
          'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000031',
          'Human decision'
        ),
        headline: translate(
          'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000032',
          'You resolve the data-policy gate'
        )
      }
    case 'relay':
      return {
        label: translate(
          'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h150000002',
          'Relay'
        ),
        headline: translate(
          'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h150000003',
          'Coordinator relays your approved policy'
        )
      }
    case 'unblocked':
      return {
        label: translate(
          'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000033',
          'Released'
        ),
        headline: translate(
          'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000034',
          'Task 1 unlocks its dependent worker'
        )
      }
    case 'blocker':
      return {
        label: translate(
          'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000035',
          'Blocker'
        ),
        headline: translate(
          'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000036',
          'A failed check returns to the coordinator'
        )
      }
    case 'recovery':
      return {
        label: translate(
          'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000037',
          'Recovery'
        ),
        headline: translate(
          'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000038',
          'Coordinator assigns a rebase and rerun'
        )
      }
    case 'complete':
      return {
        label: translate(
          'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000039',
          'Coordinator result'
        ),
        headline: translate(
          'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000040',
          '2 tasks complete · 1 human decision · 1 recovery'
        )
      }
  }
}
