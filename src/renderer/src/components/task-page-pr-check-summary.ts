import type { GitHubPRCheckSummary, PRCheckDetail } from '../../../shared/types'

function getCheckConclusion(check: PRCheckDetail): NonNullable<PRCheckDetail['conclusion']> {
  return check.conclusion ?? 'pending'
}

// Why: dedupe the fail-closed warning — check lists re-derive on every poll.
const warnedUnknownConclusions = new Set<string>()

export function deriveTaskPagePRCheckSummary(checks: PRCheckDetail[]): GitHubPRCheckSummary {
  if (checks.length === 0) {
    return { state: 'none', total: 0, passed: 0, failed: 0, pending: 0 }
  }

  let passed = 0
  let failed = 0
  let pending = 0

  for (const check of checks) {
    // Why: (string & {}) keeps the switch lint-exhaustive over the declared union while
    // the IPC/relay input stays open — a version-skewed producer can send unknown values.
    const conclusion = getCheckConclusion(check) as
      | NonNullable<PRCheckDetail['conclusion']>
      | (string & {})
    switch (conclusion) {
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
      default: {
        // Why: fail closed — an uncounted unknown would leave failed=0/pending=0
        // and render a false green 'success' for a state we don't understand.
        if (!warnedUnknownConclusions.has(conclusion)) {
          warnedUnknownConclusions.add(conclusion)
          console.warn(
            `[task-page:checks] unknown check conclusion counted as failed: ${conclusion}`
          )
        }
        failed += 1
        break
      }
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
