import { readFileSync } from 'node:fs'
import { beforeAll, describe, expect, it } from 'vitest'
import { planCommitMessageGeneration } from './commit-message-plan'
import { initGitWasmForTestFromBytes } from './git-line-stats'

// Ported from the deleted src/shared/commit-message-plan.test.ts: the same golden
// expectations now run THROUGH the Rust orca-agents core via wasm (the renderer's
// dry-run preview path). The main process runs the identical planner via napi.

const preInit = planCommitMessageGeneration({ agentId: 'claude', model: 'sonnet' }, 'PROMPT')

beforeAll(() => {
  initGitWasmForTestFromBytes(readFileSync(new URL('./orca_git_wasm_bg.wasm', import.meta.url)))
})

describe('planCommitMessageGeneration (orca-agents wasm)', () => {
  it('returns null before the wasm is ready (dialog leaves Run disabled)', () => {
    expect(preInit).toBeNull()
  })

  it('plans Claude non-interactive generation with the prompt on stdin only', () => {
    expect(
      planCommitMessageGeneration({ agentId: 'claude', model: 'sonnet', thinkingLevel: 'high' }, 'PROMPT')
    ).toEqual({
      ok: true,
      plan: {
        binary: 'claude',
        args: ['-p', '--output-format', 'text', '--model', 'sonnet', '--permission-mode', 'plan', '--effort', 'high'],
        stdinPayload: 'PROMPT',
        label: 'Claude'
      }
    })
  })

  it('plans OpenCode run with prompt on stdin and model variant', () => {
    expect(
      planCommitMessageGeneration(
        { agentId: 'opencode', model: 'opencode/gpt-5.4-mini', thinkingLevel: 'high' },
        'PROMPT'
      )
    ).toEqual({
      ok: true,
      plan: {
        binary: 'opencode',
        args: ['run', '--model', 'opencode/gpt-5.4-mini', '--agent', 'build', '--format', 'default', '--variant', 'high'],
        stdinPayload: 'PROMPT',
        label: 'OpenCode'
      }
    })
  })

  it('keeps OpenCode preset command overrides while sending the prompt on stdin', () => {
    expect(
      planCommitMessageGeneration(
        { agentId: 'opencode', model: 'opencode/gpt-5.4-mini', agentCommandOverride: 'npx opencode' },
        'PROMPT'
      )
    ).toEqual({
      ok: true,
      plan: {
        binary: 'npx',
        args: ['opencode', 'run', '--model', 'opencode/gpt-5.4-mini', '--agent', 'build', '--format', 'default'],
        stdinPayload: 'PROMPT',
        label: 'OpenCode'
      }
    })
  })

  it('plans Amp execute generation without the removed archive flag', () => {
    expect(
      planCommitMessageGeneration({ agentId: 'amp', model: 'large', thinkingLevel: 'medium' }, 'PROMPT')
    ).toEqual({
      ok: true,
      plan: {
        binary: 'amp',
        args: ['--execute', '--no-notifications', '--no-ide', '--no-jetbrains', '--mode', 'large', '--effort', 'medium'],
        stdinPayload: 'PROMPT',
        label: 'Amp'
      }
    })
  })

  it('allows discovered dynamic models that are not in the seed catalog', () => {
    expect(
      planCommitMessageGeneration({ agentId: 'cursor', model: 'gpt-5.2', thinkingLevel: 'xhigh' }, 'PROMPT')
    ).toEqual({
      ok: true,
      plan: {
        binary: 'cursor-agent',
        args: ['--print', '--mode', 'ask', '--trust', '--output-format', 'text', '--model', 'gpt-5.2', 'PROMPT'],
        stdinPayload: null,
        label: 'Cursor'
      }
    })
  })

  it('plans Codex exec as non-interactive read-only generation with the prompt on stdin only', () => {
    expect(
      planCommitMessageGeneration({ agentId: 'codex', model: 'gpt-5.4-mini', thinkingLevel: 'medium' }, 'PROMPT')
    ).toEqual({
      ok: true,
      plan: {
        binary: 'codex',
        args: ['exec', '--ephemeral', '--skip-git-repo-check', '-s', 'read-only', '--model', 'gpt-5.4-mini', '-c', 'model_reasoning_effort=medium'],
        stdinPayload: 'PROMPT',
        label: 'Codex'
      }
    })
  })

  it('uses preset agent command overrides as the spawn command prefix', () => {
    expect(
      planCommitMessageGeneration({ agentId: 'codex', model: 'gpt-5.4-mini', agentCommandOverride: 'npx codex' }, 'PROMPT')
    ).toMatchObject({
      ok: true,
      plan: {
        binary: 'npx',
        args: ['codex', 'exec', '--ephemeral', '--skip-git-repo-check', '-s', 'read-only', '--model', 'gpt-5.4-mini'],
        stdinPayload: 'PROMPT'
      }
    })
  })

  it('appends per-action CLI arguments after the built-in model args for stdin agents', () => {
    expect(
      planCommitMessageGeneration(
        { agentId: 'codex', model: 'gpt-5.4-mini', agentArgs: '--model gpt-5.5 --sandbox read-only' },
        'PROMPT'
      )
    ).toMatchObject({
      ok: true,
      plan: {
        args: ['exec', '--ephemeral', '--skip-git-repo-check', '-s', 'read-only', '--model', 'gpt-5.4-mini', '--model', 'gpt-5.5', '--sandbox', 'read-only'],
        stdinPayload: 'PROMPT'
      }
    })
  })

  it('appends per-action CLI arguments for stdin agents', () => {
    expect(
      planCommitMessageGeneration(
        { agentId: 'opencode', model: 'opencode/gpt-5.4-mini', agentArgs: '--model opencode/gpt-5.5' },
        'PROMPT'
      )
    ).toMatchObject({
      ok: true,
      plan: {
        args: ['run', '--model', 'opencode/gpt-5.4-mini', '--agent', 'build', '--format', 'default', '--model', 'opencode/gpt-5.5'],
        stdinPayload: 'PROMPT'
      }
    })
  })

  it('keeps custom per-action CLI arguments before a positional prompt', () => {
    expect(
      planCommitMessageGeneration(
        { agentId: 'custom', model: '', customAgentCommand: 'agent --message {prompt}', agentArgs: '--model gpt-5.5' },
        'PROMPT'
      )
    ).toEqual({
      ok: true,
      plan: { binary: 'agent', args: ['--message', '--model', 'gpt-5.5', 'PROMPT'], stdinPayload: null, label: 'agent' }
    })
  })

  it('appends custom per-action CLI arguments when the prompt is sent on stdin', () => {
    expect(
      planCommitMessageGeneration(
        { agentId: 'custom', model: '', customAgentCommand: 'agent --message', agentArgs: '--model gpt-5.5' },
        'PROMPT'
      )
    ).toMatchObject({
      ok: true,
      plan: { args: ['--message', '--model', 'gpt-5.5'], stdinPayload: 'PROMPT' }
    })
  })

  it('rejects invalid per-action CLI arguments before spawning', () => {
    expect(
      planCommitMessageGeneration({ agentId: 'claude', model: 'haiku', agentArgs: '--model "unterminated' }, 'PROMPT')
    ).toEqual({ ok: false, error: 'CLI arguments are invalid: Unclosed quote in command template.' })
  })

  it('rejects invalid preset agent command overrides before spawning', () => {
    expect(
      planCommitMessageGeneration({ agentId: 'claude', model: 'haiku', agentCommandOverride: 'claude "unterminated' }, 'PROMPT')
    ).toEqual({ ok: false, error: 'Agent command override is invalid: Unclosed quote in command template.' })
  })
})
