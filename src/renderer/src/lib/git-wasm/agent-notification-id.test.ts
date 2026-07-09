import { readFileSync } from 'node:fs'
import { beforeAll, describe, expect, it } from 'vitest'
import { buildAgentNotificationId } from './agent-notification-id'
import { initGitWasmForTestFromBytes } from './git-line-stats'

// Ported from the deleted src/shared/agent-notification-id.test.ts: the same
// golden id derivation now runs THROUGH the Rust orca-core via wasm. The
// pre-ready degrade (null so the consumers' null-guards skip the id) can only be
// observed here — the parity vectors pin the ready-state goldens.

const preInit = buildAgentNotificationId({
  worktreeId: 'repo::/Users/me/orca/workspaces/feature',
  paneKey: 'tab-1:11111111-1111-4111-8111-111111111111',
  stateStartedAt: 1780000000123
})

beforeAll(() => {
  initGitWasmForTestFromBytes(readFileSync(new URL('./orca_git_wasm_bg.wasm', import.meta.url)))
})

describe('buildAgentNotificationId wasm wrapper — before ready', () => {
  it('degrades to null so the consumers null-guards skip the id', () => {
    expect(preInit).toBeNull()
  })
})

describe('buildAgentNotificationId (orca-core wasm)', () => {
  it('builds a stable id for the same agent event metadata', () => {
    const args = {
      worktreeId: 'repo::/Users/me/orca/workspaces/feature',
      paneKey: 'tab-1:11111111-1111-4111-8111-111111111111',
      stateStartedAt: 1780000000123
    }

    expect(buildAgentNotificationId(args)).toBe(buildAgentNotificationId(args))
  })

  it('percent-encodes reserved chars and truncates fractional start times', () => {
    expect(
      buildAgentNotificationId({ worktreeId: 'wt/a b&c?', paneKey: 'pane#1', stateStartedAt: 0 })
    ).toBe('agent:wt%2Fa%20b%26c%3F:pane%231:0')
    expect(
      buildAgentNotificationId({
        worktreeId: 'repo::/Users/me/orca/workspaces/feature',
        paneKey: 'tab-1:11111111-1111-4111-8111-111111111111',
        stateStartedAt: 1780000000456.5
      })
    ).toBe(
      'agent:repo%3A%3A%2FUsers%2Fme%2Forca%2Fworkspaces%2Ffeature:tab-1%3A11111111-1111-4111-8111-111111111111:1780000000456'
    )
  })

  it('changes when the agent state start time changes', () => {
    const base = {
      worktreeId: 'repo::/Users/me/orca/workspaces/feature',
      paneKey: 'tab-1:11111111-1111-4111-8111-111111111111'
    }

    expect(buildAgentNotificationId({ ...base, stateStartedAt: 1780000000123 })).not.toBe(
      buildAgentNotificationId({ ...base, stateStartedAt: 1780000000456 })
    )
  })

  it('returns null when required fields are missing', () => {
    expect(
      buildAgentNotificationId({
        paneKey: 'tab-1:11111111-1111-4111-8111-111111111111',
        stateStartedAt: 1780000000123
      })
    ).toBeNull()
    expect(
      buildAgentNotificationId({
        worktreeId: 'repo::/Users/me/orca/workspaces/feature',
        stateStartedAt: 1780000000123
      })
    ).toBeNull()
    expect(
      buildAgentNotificationId({
        worktreeId: 'repo::/Users/me/orca/workspaces/feature',
        paneKey: 'tab-1:11111111-1111-4111-8111-111111111111'
      })
    ).toBeNull()
  })
})
