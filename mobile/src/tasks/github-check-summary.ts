import type { PRCheckDetail } from '../../../src/shared/types'

export type GitHubCheckLike = {
  status: string
  conclusion?: string | null
}

export type GitHubCheckSummary = {
  state: 'success' | 'failure' | 'pending' | 'none'
  total: number
  passed: number
  failed: number
  pending: number
}

type CheckOutcome = 'passed' | 'failed' | 'pending'

function isPendingStatus(check: GitHubCheckLike): boolean {
  return check.status === 'queued' || check.status === 'in_progress'
}

function classifyCheck(check: GitHubCheckLike): CheckOutcome {
  // Why: narrow to the desktop RPC's normalized union so the switch is compile-time exhaustive.
  const conclusion = (check.conclusion ?? 'pending') as NonNullable<PRCheckDetail['conclusion']>
  switch (conclusion) {
    case 'success':
    case 'neutral':
    case 'skipped':
      return 'passed'
    case 'failure':
    case 'timed_out':
    case 'cancelled':
    // Why: action_required (e.g. a workflow awaiting approval) blocks merge; it
    // must count as failed so the summary never reads "passing" while blocked.
    case 'action_required':
      return 'failed'
    case 'pending':
      return 'pending'
    default: {
      if (isPendingStatus(check)) {
        return 'pending'
      }
      // Why: conclusion is untyped over the wire; fail closed on unknown values so
      // the summary never silently reads green for a state we don't understand.
      const unknown: never = conclusion
      console.warn('[github-check-summary] unknown check conclusion', { conclusion: unknown })
      return 'failed'
    }
  }
}

export function buildGitHubCheckSummary(checks: GitHubCheckLike[]): GitHubCheckSummary {
  let passed = 0
  let failed = 0
  let pending = 0

  for (const check of checks) {
    switch (classifyCheck(check)) {
      case 'passed':
        passed += 1
        break
      case 'failed':
        failed += 1
        break
      case 'pending':
        pending += 1
        break
    }
  }

  const total = checks.length
  const state = total === 0 ? 'none' : failed > 0 ? 'failure' : pending > 0 ? 'pending' : 'success'

  return { state, total, passed, failed, pending }
}
