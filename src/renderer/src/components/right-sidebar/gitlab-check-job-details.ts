// Maps a GitLab job trace onto the provider-neutral PRCheckRunDetails shape the
// Checks panel expands inline, so GitLab rows load details like GitHub check
// runs (#7732). The row's checkRunId carries the GitLab job id.
import { sliceCheckLogTail } from '../../../../shared/check-job-log-tail-slice'
import type { PRCheckDetail, PRCheckRunDetails } from '../../../../shared/types'

// Why: raw traces carry ANSI colors plus GitLab's collapsible-section markers
// (`section_start:<ts>:<name>\r\x1b[0K`); both render as noise in a plain <pre>.
// eslint-disable-next-line no-control-regex
const ANSI_ESCAPE_PATTERN = /\u001b(?:\[[0-9;?]*[ -/]*[@-~]|\][^\u0007\u001b]*(?:\u0007|\u001b\\)?)/g
const GITLAB_SECTION_MARKER_PATTERN = /section_(?:start|end):\d+:[^\r\n]*\r?/g

export function stripGitLabJobTraceMarkup(trace: string): string {
  return trace.replace(ANSI_ESCAPE_PATTERN, '').replace(GITLAB_SECTION_MARKER_PATTERN, '')
}

export function gitLabJobTraceToCheckRunDetails(
  check: PRCheckDetail,
  trace: string
): PRCheckRunDetails {
  const logTail = sliceCheckLogTail(stripGitLabJobTraceMarkup(trace)).trim()
  return {
    name: check.name,
    status: check.status,
    conclusion: check.conclusion,
    url: check.url,
    detailsUrl: check.url,
    startedAt: null,
    completedAt: null,
    title: null,
    summary: null,
    text: null,
    annotations: [],
    jobs: [
      {
        id: check.checkRunId ?? null,
        name: check.name,
        status: check.status,
        conclusion: check.conclusion,
        startedAt: null,
        completedAt: null,
        url: check.url,
        logTail: logTail.length > 0 ? logTail : null,
        steps: []
      }
    ]
  }
}
