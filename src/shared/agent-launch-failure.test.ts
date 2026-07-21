import { describe, expect, it } from 'vitest'
import {
  AGENT_LAUNCH_FAILURE_IMMEDIATE_EXIT_WINDOW_MS,
  AGENT_LAUNCH_FAILURE_SEED_WINDOW_MS,
  agentLaunchFailureMessage,
  isAgentLaunchFailureExit
} from './agent-launch-failure'

describe('isAgentLaunchFailureExit', () => {
  it('classifies command-not-found exits near the launch seed', () => {
    expect(isAgentLaunchFailureExit({ exitCode: 127, msSinceStatusSeed: 1_500 })).toBe(true)
    expect(isAgentLaunchFailureExit({ exitCode: 126, msSinceStatusSeed: 1_500 })).toBe(true)
    expect(
      isAgentLaunchFailureExit({
        exitCode: 127,
        msSinceStatusSeed: AGENT_LAUNCH_FAILURE_SEED_WINDOW_MS
      })
    ).toBe(true)
  })

  it('does not classify a late 127 from an unrelated shell command', () => {
    expect(
      isAgentLaunchFailureExit({
        exitCode: 127,
        msSinceStatusSeed: AGENT_LAUNCH_FAILURE_SEED_WINDOW_MS + 1
      })
    ).toBe(false)
  })

  it('classifies other non-zero exits only when immediate', () => {
    expect(isAgentLaunchFailureExit({ exitCode: 1, msSinceStatusSeed: 2_000 })).toBe(true)
    expect(
      isAgentLaunchFailureExit({
        exitCode: 1,
        msSinceStatusSeed: AGENT_LAUNCH_FAILURE_IMMEDIATE_EXIT_WINDOW_MS + 1
      })
    ).toBe(false)
  })

  it('never classifies success, unknown, or pre-seed exits', () => {
    expect(isAgentLaunchFailureExit({ exitCode: 0, msSinceStatusSeed: 100 })).toBe(false)
    expect(isAgentLaunchFailureExit({ exitCode: null, msSinceStatusSeed: 100 })).toBe(false)
    expect(isAgentLaunchFailureExit({ exitCode: 127, msSinceStatusSeed: -1 })).toBe(false)
  })
})

describe('agentLaunchFailureMessage', () => {
  it('names the agent and the missing-command cause', () => {
    expect(agentLaunchFailureMessage(127, 'codex')).toBe(
      'Codex failed to launch: command not found (exit 127). Install the CLI on this host and try again.'
    )
    expect(agentLaunchFailureMessage(126, 'claude')).toContain('command not executable (exit 126)')
    expect(agentLaunchFailureMessage(9, 'claude')).toBe('Claude failed to launch (exit 9).')
    expect(agentLaunchFailureMessage(127, undefined)).toContain('Agent failed to launch')
  })
})
