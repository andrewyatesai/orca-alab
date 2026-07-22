import type { PRInfo, IssueInfo, CheckStatus, PRCheckDetail } from '../../shared/types'
import { latestRollupEntriesByName } from './check-run-dedup'

// ── REST API check-runs mapping ───────────────────────────────────────
// The REST check-runs endpoint returns separate status + conclusion fields
// (unlike gh pr checks which merges them into a single "state" string).

export function mapCheckRunRESTStatus(status: string): PRCheckDetail['status'] {
  const s = status?.toLowerCase()
  if (s === 'queued') {
    return 'queued'
  }
  if (s === 'in_progress') {
    return 'in_progress'
  }
  return 'completed'
}

// Why: REST check-runs conclusion domain (GitHub API) — the switch below must stay exhaustive over it.
type CheckRunRESTConclusion =
  | 'success'
  | 'failure'
  | 'cancelled'
  | 'timed_out'
  | 'skipped'
  | 'neutral'
  | 'action_required'
  | 'stale'
  | 'startup_failure'

export function mapCheckRunRESTConclusion(
  status: string,
  conclusion: string | null
): PRCheckDetail['conclusion'] {
  if (status?.toLowerCase() !== 'completed') {
    return 'pending'
  }
  if (!conclusion) {
    return null
  }
  // Why: (string & {}) keeps the switch lint-exhaustive over the declared union while the API input stays open.
  const c = conclusion.toLowerCase() as CheckRunRESTConclusion | (string & {})
  switch (c) {
    case 'success':
      return 'success'
    case 'failure':
    case 'stale':
    case 'startup_failure':
      return 'failure'
    case 'cancelled':
      return 'cancelled'
    case 'timed_out':
      return 'timed_out'
    case 'skipped':
      return 'skipped'
    case 'neutral':
      return 'neutral'
    case 'action_required':
      return 'action_required'
    default: {
      // Why: unknown-completed counts failed (deriveWorkItemCheckSummary rule) — a silent null renders as perpetual Pending.
      console.warn(`[github:checks] unmapped REST check-run conclusion: ${c}`)
      return 'failure'
    }
  }
}

// ── REST API commit status mapping ──────────────────────────────────────
// Legacy Jenkins/Prow integrations report commit statuses, not check runs.

export function mapCommitStatusRESTStatus(state: string): PRCheckDetail['status'] {
  const s = state?.toLowerCase()
  return s === 'pending' ? 'queued' : 'completed'
}

export function mapCommitStatusRESTConclusion(state: string): PRCheckDetail['conclusion'] {
  const s = state?.toLowerCase()
  if (s === 'success') {
    return 'success'
  }
  if (s === 'failure' || s === 'error') {
    return 'failure'
  }
  if (s === 'pending') {
    return 'pending'
  }
  return null
}

// ── gh pr checks mapping (single "state" string) ─────────────────────

export function mapCheckStatus(state: string): PRCheckDetail['status'] {
  const s = state?.toUpperCase()
  if (s === 'PENDING' || s === 'QUEUED') {
    return 'queued'
  }
  if (s === 'IN_PROGRESS') {
    return 'in_progress'
  }
  return 'completed'
}

// Why: gh pr checks flattens CheckRun conclusion/status and commit-status StatusContext
// state into one string; this is the known domain the switch must stay exhaustive over.
type GhPrChecksState =
  | 'SUCCESS'
  | 'PASS'
  | 'FAILURE'
  | 'FAIL'
  | 'ERROR'
  | 'ACTION_REQUIRED'
  | 'STALE'
  | 'STARTUP_FAILURE'
  | 'CANCELLED'
  | 'TIMED_OUT'
  | 'SKIPPED'
  | 'PENDING'
  | 'QUEUED'
  | 'IN_PROGRESS'
  | 'EXPECTED'
  | 'NEUTRAL'

export function mapCheckConclusion(state: string): PRCheckDetail['conclusion'] {
  const raw = state?.toUpperCase()
  if (!raw) {
    return null
  }
  // Why: (string & {}) keeps the switch lint-exhaustive over the declared union while the gh input stays open.
  const s = raw as GhPrChecksState | (string & {})
  switch (s) {
    case 'SUCCESS':
    case 'PASS':
      return 'success'
    case 'FAILURE':
    case 'FAIL':
    // Why: commit-status contexts (Jenkins/Prow) report ERROR as their terminal failure — keep in sync with mapCommitStatusRESTConclusion.
    case 'ERROR':
    case 'STALE':
    case 'STARTUP_FAILURE':
      return 'failure'
    case 'ACTION_REQUIRED':
      return 'action_required'
    case 'CANCELLED':
      return 'cancelled'
    case 'TIMED_OUT':
      return 'timed_out'
    case 'SKIPPED':
      return 'skipped'
    case 'PENDING':
    case 'QUEUED':
    case 'IN_PROGRESS':
    // Why: EXPECTED is a required context that hasn't reported yet — pending, not done.
    case 'EXPECTED':
      return 'pending'
    case 'NEUTRAL':
      return 'neutral'
    default: {
      // Why: mapCheckStatus buckets anything outside its pending set as completed; an
      // unknown completed state must read failed (deriveWorkItemCheckSummary rule), never a silent null→Pending.
      console.warn(`[github:checks] unmapped gh pr checks state: ${s}`)
      return mapCheckStatus(state) === 'completed' ? 'failure' : 'pending'
    }
  }
}

export function mapPRState(state: string, isDraft?: boolean): PRInfo['state'] {
  const s = state?.toUpperCase()
  if (s === 'MERGED') {
    return 'merged'
  }
  if (s === 'CLOSED') {
    return 'closed'
  }
  if (isDraft) {
    return 'draft'
  }
  return 'open'
}

// ── Issue mapping ────────────────────────────────────────────────────
// REST API returns html_url + lowercase state; gh issue view returns url + mixed-case state.
// This helper normalises both shapes into IssueInfo.

export function mapIssueInfo(data: {
  number: number
  title: string
  state: string
  url?: string
  html_url?: string
  labels?: { name: string }[]
}): IssueInfo {
  return {
    number: data.number,
    title: data.title,
    state: data.state?.toLowerCase() === 'open' ? 'open' : 'closed',
    url: data.html_url ?? data.url ?? '',
    labels: (data.labels || []).map((l) => l.name)
  }
}

// Why: GraphQL enum domains for statusCheckRollup entries — CheckStatusState /
// CheckConclusionState (check runs) and StatusState (commit-status contexts).
// The switches below must stay exhaustive over them; unknown values log and never read green.
type RollupCheckRunStatus =
  | 'QUEUED'
  | 'IN_PROGRESS'
  | 'COMPLETED'
  | 'WAITING'
  | 'PENDING'
  | 'REQUESTED'
type RollupCheckRunConclusion =
  | 'SUCCESS'
  | 'FAILURE'
  | 'TIMED_OUT'
  | 'CANCELLED'
  | 'ACTION_REQUIRED'
  | 'STARTUP_FAILURE'
  | 'STALE'
  | 'SKIPPED'
  | 'NEUTRAL'
type RollupStatusContextState = 'EXPECTED' | 'ERROR' | 'FAILURE' | 'PENDING' | 'SUCCESS'

type RollupEntryVerdict = 'failure' | 'pending' | 'success' | 'none'

function classifyStatusContextState(state: string): RollupEntryVerdict {
  const s = state as RollupStatusContextState | (string & {})
  switch (s) {
    case 'SUCCESS':
      return 'success'
    case 'FAILURE':
    case 'ERROR':
      return 'failure'
    case 'PENDING':
    // Why: EXPECTED is a required context that hasn't reported yet — it blocks merge, so it must not read green.
    case 'EXPECTED':
      return 'pending'
    default: {
      console.warn(`[github:checks] unknown rollup status-context state: ${s}`)
      return 'none'
    }
  }
}

function classifyCheckRunEntry(
  status: string | undefined,
  conclusion: string | undefined
): RollupEntryVerdict {
  if (conclusion) {
    const c = conclusion as RollupCheckRunConclusion | (string & {})
    switch (c) {
      case 'SUCCESS':
        return 'success'
      case 'FAILURE':
      case 'TIMED_OUT':
      case 'CANCELLED':
      // Why: action_required (e.g. an unapproved workflow run) blocks merge until
      // someone acts; treat it as needs-attention rather than a silent pass.
      case 'ACTION_REQUIRED':
      // Why: keep in sync with mapCheckConclusion — both are terminal failures (upstream #4605).
      case 'STARTUP_FAILURE':
      case 'STALE':
        return 'failure'
      case 'SKIPPED':
      case 'NEUTRAL':
        return 'none'
      default: {
        console.warn(`[github:checks] unknown rollup conclusion: ${c}`)
        return 'none'
      }
    }
  }
  const s = status as RollupCheckRunStatus | (string & {}) | undefined
  switch (s) {
    case 'QUEUED':
    case 'IN_PROGRESS':
    case 'PENDING':
    // Why: WAITING (deployment protection) and REQUESTED are not-yet-complete — they must not read green.
    case 'WAITING':
    case 'REQUESTED':
      return 'pending'
    // Why: COMPLETED with no conclusion is an API edge with no verdict to derive.
    case 'COMPLETED':
    case undefined:
      return 'none'
    default: {
      console.warn(`[github:checks] unknown rollup check-run status: ${s}`)
      return 'none'
    }
  }
}

export function deriveCheckStatus(rollup: unknown[] | null | undefined): CheckStatus {
  if (!rollup || !Array.isArray(rollup) || rollup.length === 0) {
    return 'neutral'
  }

  let hasFailure = false
  let hasPending = false
  let hasSuccess = false

  // Collapse superseded runs first so a stale CANCELLED/FAILURE doesn't outvote a later SUCCESS.
  const latest = latestRollupEntriesByName(rollup)
  for (const check of latest as { status?: string; conclusion?: string; state?: string }[]) {
    const state = check.state?.toUpperCase()
    const verdict = state
      ? classifyStatusContextState(state)
      : classifyCheckRunEntry(check.status?.toUpperCase(), check.conclusion?.toUpperCase())
    if (verdict === 'failure') {
      hasFailure = true
    } else if (verdict === 'pending') {
      hasPending = true
    } else if (verdict === 'success') {
      hasSuccess = true
    }
  }

  if (hasFailure) {
    return 'failure'
  }
  if (hasPending) {
    return 'pending'
  }
  // Why: a rollup with no succeeded run (e.g. all skipped) must not read green —
  // match GitLab derivePipelineStatus's neutral for the same inputs.
  return hasSuccess ? 'success' : 'neutral'
}
