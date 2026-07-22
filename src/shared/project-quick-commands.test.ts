import { describe, expect, it } from 'vitest'
import {
  isProjectQuickCommand,
  projectQuickCommandsForRepo,
  projectQuickCommandsNotOverriddenByLocal
} from './project-quick-commands'
import type { TerminalQuickCommand } from './types'

describe('projectQuickCommandsForRepo', () => {
  it('derives stable orcaYaml-prefixed ids scoped to the repo for both variants', () => {
    const commands = projectQuickCommandsForRepo('repo-1', {
      quickCommands: [
        { label: 'Dev server', command: 'npm run dev' },
        { label: 'Insert only', command: 'git status', appendEnter: false },
        { label: 'Investigate', action: 'agent-prompt', agent: 'claude', prompt: 'Investigate' }
      ]
    })

    expect(commands).toEqual([
      {
        id: 'orcaYaml:repo-1:0',
        label: 'Dev server',
        scope: { type: 'repo', repoId: 'repo-1' },
        action: 'terminal-command',
        command: 'npm run dev',
        appendEnter: true
      },
      {
        id: 'orcaYaml:repo-1:1',
        label: 'Insert only',
        scope: { type: 'repo', repoId: 'repo-1' },
        action: 'terminal-command',
        command: 'git status',
        appendEnter: false
      },
      {
        id: 'orcaYaml:repo-1:2',
        label: 'Investigate',
        scope: { type: 'repo', repoId: 'repo-1' },
        action: 'agent-prompt',
        agent: 'claude',
        prompt: 'Investigate'
      }
    ])
    expect(commands.every(isProjectQuickCommand)).toBe(true)
  })

  it('drops agent entries whose agent Orca cannot inject a prompt into', () => {
    const commands = projectQuickCommandsForRepo('repo-1', {
      quickCommands: [
        { label: 'Unknown agent', action: 'agent-prompt', agent: 'not-an-agent', prompt: 'x' },
        // Why: real agent, but its stdin-after-start injection mode is unsupported for quick commands.
        { label: 'Teams', action: 'agent-prompt', agent: 'claude-agent-teams', prompt: 'x' },
        { label: 'Kept', command: 'echo ok' }
      ]
    })
    expect(commands.map((command) => command.label)).toEqual(['Kept'])
  })

  it('returns empty for null hooks', () => {
    expect(projectQuickCommandsForRepo('repo-1', null)).toEqual([])
  })
})

describe('projectQuickCommandsNotOverriddenByLocal', () => {
  it('local quick command overrides a project quick command with the same label', () => {
    const local: TerminalQuickCommand[] = [
      {
        id: 'user-1',
        label: 'Dev server',
        scope: { type: 'repo', repoId: 'repo-1' },
        action: 'terminal-command',
        command: 'npm run dev -- --local',
        appendEnter: true
      }
    ]
    const project = projectQuickCommandsForRepo('repo-1', {
      quickCommands: [
        { label: 'Dev server', command: 'npm run dev' },
        { label: 'Lint', command: 'npm run lint' }
      ]
    })

    expect(
      projectQuickCommandsNotOverriddenByLocal(local, project).map((command) => command.label)
    ).toEqual(['Lint'])
  })

  it('compares labels after trimming', () => {
    const local: TerminalQuickCommand[] = [
      {
        id: 'user-1',
        label: ' Dev server ',
        scope: { type: 'repo', repoId: 'repo-1' },
        action: 'terminal-command',
        command: 'npm run dev',
        appendEnter: true
      }
    ]
    const project = projectQuickCommandsForRepo('repo-1', {
      quickCommands: [{ label: 'Dev server', command: 'npm run dev' }]
    })
    expect(projectQuickCommandsNotOverriddenByLocal(local, project)).toEqual([])
  })
})

describe('isProjectQuickCommand', () => {
  it('recognizes only the orcaYaml id prefix', () => {
    expect(isProjectQuickCommand({ id: 'orcaYaml:repo-1:0' })).toBe(true)
    expect(isProjectQuickCommand({ id: 'user-command' })).toBe(false)
  })
})
