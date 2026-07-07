import { readFileSync } from 'node:fs'
import { beforeAll, describe, expect, it } from 'vitest'
import {
  planSourceControlCommitMessageGeneration,
  planSourceControlTextGeneration
} from './source-control-generation-plan'
import { initGitWasmForTestFromBytes } from './git-wasm/git-line-stats'

// The planner preview derives its plan through the Rust orca-agents core via
// wasm; init it synchronously from the committed bytes (else it returns null).
beforeAll(() => {
  initGitWasmForTestFromBytes(
    readFileSync(new URL('./git-wasm/orca_git_wasm_bg.wasm', import.meta.url))
  )
})

describe('planSourceControlCommitMessageGeneration', () => {
  it('catches empty custom commands without invoking an agent', () => {
    expect(
      planSourceControlCommitMessageGeneration({
        agentId: 'custom',
        model: '',
        customAgentCommand: ''
      })
    ).toEqual({
      ok: false,
      error: 'Custom command is empty. Add one in Settings → Git → AI Commit Messages.'
    })
  })

  it('rejects command templates that render empty input', () => {
    expect(
      planSourceControlCommitMessageGeneration({
        agentId: 'codex',
        model: 'gpt-5.5',
        commandInputTemplate: ''
      })
    ).toEqual({ ok: false, error: 'Command input is empty.' })
  })

  it('plans known agents and includes renderer-only caveats', () => {
    const result = planSourceControlCommitMessageGeneration({
      agentId: 'codex',
      model: 'gpt-5.5',
      thinkingLevel: 'low'
    })

    expect(result?.ok && result.commandLabel).toContain('codex exec')
    expect(result?.ok && result.delivery).toContain('stdin')
    expect(result?.ok && result.caveat).toContain('Windows .cmd')
  })

  it('plans pull-request generation with pull-request variables', () => {
    const result = planSourceControlTextGeneration('pullRequest', {
      agentId: 'codex',
      model: 'gpt-5.5',
      commandInputTemplate: '{basePrompt}\n\nReview {changedFiles}'
    })

    expect(result?.ok && result.commandLabel).toContain('codex exec')
  })

  it('shows per-action CLI arguments in dry-run command labels', () => {
    const result = planSourceControlTextGeneration('pullRequest', {
      agentId: 'codex',
      model: 'gpt-5.5',
      agentArgs: '--model gpt-5.4',
      commandInputTemplate: '{basePrompt}'
    })

    expect(result?.ok && result.commandLabel).toContain('--model gpt-5.4')
  })
})
