import { translate } from '@/i18n/i18n'
import type { OrchestrationPhase } from './orchestration-types'

export function getOrchestrationLedgerCopy(phase: OrchestrationPhase): {
  dependency: string
  decision: string
  recovery: string
} {
  return {
    dependency: dependencyCopy(phase),
    decision: decisionCopy(phase),
    recovery: recoveryCopy(phase)
  }
}

function dependencyCopy(phase: OrchestrationPhase): string {
  if (['unblocked', 'blocker', 'recovery', 'complete'].includes(phase)) {
    return translate(
      'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000041',
      'Released · schema ready'
    )
  }
  return translate(
    'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000042',
    'Task 2 waits on Task 1'
  )
}

function decisionCopy(phase: OrchestrationPhase): string {
  if (phase === 'question') {
    return translate(
      'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000043',
      'Answer required'
    )
  }
  if (['decision', 'relay', 'unblocked', 'blocker', 'recovery', 'complete'].includes(phase)) {
    return translate(
      'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000044',
      'Human-approved · false + backfill'
    )
  }
  return translate(
    'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000045',
    'Gate pending'
  )
}

function recoveryCopy(phase: OrchestrationPhase): string {
  if (phase === 'complete') {
    return translate(
      'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000046',
      'Recovered · checks pass'
    )
  }
  if (phase === 'recovery') {
    return translate(
      'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000047',
      'Rebase + rerun active'
    )
  }
  if (phase === 'blocker') {
    return translate(
      'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000048',
      'Coordinator owns blocker'
    )
  }
  return translate(
    'auto.components.feature.wall.agents.orchestration.OrchestrationPage.h130000049',
    'Watching worker checks'
  )
}
