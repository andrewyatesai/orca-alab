import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { beforeAll, describe, expect, it } from 'vitest'

import type { LinearIssue } from '../../../shared/types'
import { initGitWasmForTestFromBytes } from '@/lib/git-wasm/git-line-stats'
import { buildLinearIssueBranchName } from './linear-issue-workspace-text'

// The generated-slug fallback runs through the Rust orca-text core via wasm;
// init it synchronously from the committed bytes (standard fork adaptation).
beforeAll(() => {
  initGitWasmForTestFromBytes(
    readFileSync(join(__dirname, '../lib/git-wasm/orca_git_wasm_bg.wasm'))
  )
})

function makeIssue(branchName?: string): LinearIssue {
  return {
    id: 'issue-1',
    identifier: 'ENG-123',
    title: 'Fix launch context handoff',
    url: 'https://linear.app/acme/issue/ENG-123/fix-launch-context-handoff',
    branchName,
    state: { name: 'Todo', type: 'unstarted', color: '#999999' },
    team: { id: 'team-1', name: 'Engineering', key: 'ENG' },
    labels: [],
    labelIds: [],
    priority: 3,
    estimate: null,
    updatedAt: '2026-05-29T12:00:00.000Z'
  }
}

describe('buildLinearIssueBranchName', () => {
  it('prefers Linear’s branch name', () => {
    expect(buildLinearIssueBranchName(makeIssue('  team/eng-123-fix-launch-context  '))).toBe(
      'team/eng-123-fix-launch-context'
    )
  })

  it('falls back to Orca’s generated workspace slug', () => {
    expect(buildLinearIssueBranchName(makeIssue())).toBe('eng-123-fix-launch-context-handoff')
    expect(buildLinearIssueBranchName(makeIssue('   '))).toBe('eng-123-fix-launch-context-handoff')
  })
})
