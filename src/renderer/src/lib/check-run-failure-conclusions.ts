// Why: GitHub job/step conclusions arrive as raw strings (types.ts PRCheckJob.conclusion),
// so the failure classifier must cover the full API domain; a stale right-sidebar copy
// missed startup_failure/stale/action_required and disagreed with the editor panel.
const warnedUnknownStates = new Set<string>()

/** Classifies a raw GitHub Actions job/step `conclusion ?? status` string as a failure. */
export function isCheckJobFailureState(state: string | null | undefined): boolean {
  switch (state) {
    case 'failure':
    case 'failed':
    case 'action_required':
    case 'cancelled':
    case 'stale':
    case 'startup_failure':
    case 'timed_out':
      return true
    case 'success':
    case 'neutral':
    case 'skipped':
    case 'pending':
    // Raw status values seen when conclusion is null:
    case 'queued':
    case 'in_progress':
    case 'waiting':
    case 'requested':
    case 'completed':
    case null:
    case undefined:
      return false
    default:
      // Why: display-focus logic — unknown values safely degrade to "show all jobs",
      // but log once so new GitHub states surface instead of silently misclassifying.
      if (!warnedUnknownStates.has(state)) {
        warnedUnknownStates.add(state)
        console.warn(`[checks] Unknown check job conclusion/status: ${state}`)
      }
      return false
  }
}
