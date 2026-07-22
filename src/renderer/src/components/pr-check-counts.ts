import type { PRCheckDetail } from '../../../shared/types'

export type PRCheckCounts = {
  passing: number
  failing: number
  needsAction: number
  pending: number
  skipped: number
  neutral: number
}

export function getCheckConclusion(check: PRCheckDetail): NonNullable<PRCheckDetail['conclusion']> {
  return check.conclusion ?? 'pending'
}

// Why: dedupe the fail-closed warning — check lists re-derive on every poll.
const warnedUnknownConclusions = new Set<string>()

export function getCheckCounts(checks: PRCheckDetail[]): PRCheckCounts {
  return checks.reduce(
    (counts, check) => {
      // Why: (string & {}) keeps the switch lint-exhaustive over the declared union while
      // the IPC/relay input stays open — a version-skewed producer can send unknown values.
      const conclusion = getCheckConclusion(check) as
        | NonNullable<PRCheckDetail['conclusion']>
        | (string & {})
      switch (conclusion) {
        case 'success':
          counts.passing += 1
          break
        case 'action_required':
          counts.needsAction += 1
          break
        case 'failure':
        case 'cancelled':
        case 'timed_out':
          counts.failing += 1
          break
        case 'skipped':
          counts.skipped += 1
          break
        case 'neutral':
          counts.neutral += 1
          break
        case 'pending':
          counts.pending += 1
          break
        default: {
          // Why: fail closed — an uncounted unknown would vanish from every bucket
          // and let the summary read green for a state we don't understand.
          if (!warnedUnknownConclusions.has(conclusion)) {
            warnedUnknownConclusions.add(conclusion)
            console.warn(`[pr-checks] unknown check conclusion counted as failing: ${conclusion}`)
          }
          counts.failing += 1
          break
        }
      }
      return counts
    },
    { passing: 0, failing: 0, needsAction: 0, pending: 0, skipped: 0, neutral: 0 }
  )
}
