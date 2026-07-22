import type { GitHubPRCheckSummary, PRCheckDetail } from '../../../shared/types'

function getCheckConclusion(check: PRCheckDetail): NonNullable<PRCheckDetail['conclusion']> {
  return check.conclusion ?? 'pending'
}

export function deriveTaskPagePRCheckSummary(checks: PRCheckDetail[]): GitHubPRCheckSummary {
  if (checks.length === 0) {
    return { state: 'none', total: 0, passed: 0, failed: 0, pending: 0 }
  }

  let passed = 0
  let failed = 0
  let pending = 0

  for (const check of checks) {
    // Why: exhaustive switch, no default — a new conclusion must fail
    // lint:switch-exhaustiveness instead of silently counting as passed.
    switch (getCheckConclusion(check)) {
      case 'success':
      case 'neutral':
      case 'skipped':
        passed += 1
        break
      case 'failure':
      case 'timed_out':
      case 'cancelled':
      // Why: action_required (e.g. a workflow awaiting approval) blocks merge; it
      // must count as failed so the summary never reads "passing" while blocked.
      case 'action_required':
        failed += 1
        break
      // Why: queued/in_progress checks land here too — their null conclusion
      // coalesces to 'pending' in getCheckConclusion.
      case 'pending':
        pending += 1
        break
    }
  }

  return {
    state: failed > 0 ? 'failure' : pending > 0 ? 'pending' : 'success',
    total: checks.length,
    passed,
    failed,
    pending
  }
}
