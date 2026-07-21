import { describe, expect, it } from 'vitest'
import {
  gitLabJobTraceToCheckRunDetails,
  stripGitLabJobTraceMarkup
} from './gitlab-check-job-details'
import type { PRCheckDetail } from '../../../../shared/types'

const ESC = '\u001b'

function check(overrides: Partial<PRCheckDetail> = {}): PRCheckDetail {
  return {
    name: 'test: unit',
    status: 'completed',
    conclusion: 'failure',
    url: 'https://gitlab.com/acme/orca/-/jobs/42',
    checkRunId: 42,
    ...overrides
  }
}

describe('stripGitLabJobTraceMarkup', () => {
  it('removes ANSI color escapes but keeps the text', () => {
    expect(stripGitLabJobTraceMarkup(`${ESC}[31mFAILED${ESC}[0m tests`)).toBe('FAILED tests')
  })

  it('removes GitLab collapsible-section markers with their trailing erase escape', () => {
    const trace = `section_start:1700000000:step_script\r${ESC}[0K$ npm test\nok\nsection_end:1700000001:step_script\r${ESC}[0K`
    expect(stripGitLabJobTraceMarkup(trace)).toBe('$ npm test\nok\n')
  })

  it('removes OSC hyperlink sequences terminated by BEL', () => {
    expect(stripGitLabJobTraceMarkup(`${ESC}]8;;https://xlink${ESC}]8;;`)).toBe('link')
  })
})

describe('gitLabJobTraceToCheckRunDetails', () => {
  it('shapes the trace into a single-job PRCheckRunDetails keyed by the check row', () => {
    const details = gitLabJobTraceToCheckRunDetails(check(), `${ESC}[32m$ npm test${ESC}[0m\nok\n`)

    expect(details.name).toBe('test: unit')
    expect(details.status).toBe('completed')
    expect(details.conclusion).toBe('failure')
    expect(details.url).toBe('https://gitlab.com/acme/orca/-/jobs/42')
    expect(details.detailsUrl).toBe('https://gitlab.com/acme/orca/-/jobs/42')
    expect(details.annotations).toEqual([])
    expect(details.jobs).toHaveLength(1)
    expect(details.jobs[0]).toMatchObject({
      id: 42,
      name: 'test: unit',
      status: 'completed',
      conclusion: 'failure',
      logTail: '$ npm test\nok',
      steps: []
    })
  })

  it('slices long traces to the shared log-tail window', () => {
    const longTrace = Array.from({ length: 500 }, (_, index) => `line ${index}`).join('\n')
    const details = gitLabJobTraceToCheckRunDetails(check(), longTrace)

    expect(details.jobs[0]?.logTail).toContain('line 499')
    expect(details.jobs[0]?.logTail).not.toContain('line 0\n')
  })

  it('reports a null logTail and job id when the trace is blank and the row has no job id', () => {
    const details = gitLabJobTraceToCheckRunDetails(check({ checkRunId: undefined }), '   \n')

    expect(details.jobs[0]?.logTail).toBeNull()
    expect(details.jobs[0]?.id).toBeNull()
  })
})
