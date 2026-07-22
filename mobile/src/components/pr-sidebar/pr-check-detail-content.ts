import type {
  PRCheckAnnotation,
  PRCheckJob,
  PRCheckRunDetails,
  PRCheckStep
} from '../../../../src/shared/types'

// Pure mapping from the github.prCheckDetails payload to the rows the mobile
// expanded check detail renders. No React/native imports so it stays unit-testable
// under the node Vitest config (KTD5). Ports the desktop CheckDetailExpanded logic
// (conclusion/title/summary + annotations + failed-job/step summary), not its JSX.

// Desktop caps the inline lists so a noisy check can't break the layout; match it.
const MAX_ANNOTATIONS = 20
const MAX_JOBS = 100

// Why: job/step conclusions arrive as raw strings; mirror the desktop classifier
// (src/renderer/src/lib/check-run-failure-conclusions.ts — mobile cannot import
// from src/renderer) so a stale copy never again misses action_required/stale/
// startup_failure and disagrees with the editor panel.
const warnedUnknownStates = new Set<string>()

function isFailureState(state: string | null | undefined): boolean {
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
        console.warn(`[pr-check-detail] Unknown check job conclusion/status: ${state}`)
      }
      return false
  }
}

export type CheckDetailAnnotation = {
  // Path:line locator (or "Annotation" when the host omits a path).
  locator: string
  level: string | null
  title: string | null
  message: string
}

export type CheckDetailStep = {
  name: string
  state: string
}

export type CheckDetailJob = {
  name: string
  state: string
  // Failed steps within the job; empty when none reported as failing.
  failedSteps: CheckDetailStep[]
  logTail: string | null
}

export type CheckDetailContent = {
  // Conclusion/title/summary lines, in render order (matches the prior mobile detail).
  summaryLines: string[]
  annotations: CheckDetailAnnotation[]
  // True when the host returned more annotations than we render.
  annotationsTruncated: boolean
  // "Failed jobs" when only failing jobs are shown, else "Jobs" (matches desktop label).
  jobsLabel: 'Failed jobs' | 'Jobs'
  jobs: CheckDetailJob[]
  jobsTruncated: boolean
}

function mapAnnotation(annotation: PRCheckAnnotation): CheckDetailAnnotation {
  const path = annotation.path ?? 'Annotation'
  const locator = annotation.startLine ? `${path}:${annotation.startLine}` : path
  return {
    locator,
    level: annotation.annotationLevel,
    title: annotation.title,
    message: annotation.message
  }
}

function mapJob(job: PRCheckJob): CheckDetailJob {
  const failedSteps = job.steps
    .filter((step: PRCheckStep) => isFailureState(step.conclusion ?? step.status))
    .map((step) => ({ name: step.name, state: step.conclusion ?? step.status ?? 'unknown' }))
  return {
    name: job.name,
    state: job.conclusion ?? job.status ?? 'unknown',
    failedSteps,
    logTail: job.logTail
  }
}

export function presentCheckDetail(details: PRCheckRunDetails): CheckDetailContent {
  const summaryLines = [
    details.conclusion ?? details.status,
    details.title,
    details.summary
  ].filter((line): line is string => typeof line === 'string' && line.trim().length > 0)

  // Why: prefer failing jobs (the actionable ones); fall back to all jobs only
  // when nothing is failing, matching the desktop panel.
  const failedJobs = details.jobs.filter((job) => isFailureState(job.conclusion ?? job.status))
  const visibleJobs = failedJobs.length > 0 ? failedJobs : details.jobs

  return {
    summaryLines,
    annotations: details.annotations.slice(0, MAX_ANNOTATIONS).map(mapAnnotation),
    annotationsTruncated: details.annotations.length > MAX_ANNOTATIONS,
    jobsLabel: failedJobs.length > 0 ? 'Failed jobs' : 'Jobs',
    jobs: visibleJobs.slice(0, MAX_JOBS).map(mapJob),
    jobsTruncated: details.jobs.length > MAX_JOBS
  }
}
