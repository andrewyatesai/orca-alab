import { describe, expect, it } from 'vitest'

import { buildAiVaultResumeCommand } from './ai-vault-types'

describe('buildAiVaultResumeCommand', () => {
  it('uses Antigravity conversation ids instead of Gemini resume flags', () => {
    expect(
      buildAiVaultResumeCommand({
        agent: 'antigravity',
        sessionId: 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee',
        cwd: '/repo/app',
        platform: 'darwin'
      })
    ).toBe("cd '/repo/app' && agy --conversation 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee'")
  })

  it('builds a self-contained cmd wrapper when no live shell is known', () => {
    expect(
      buildAiVaultResumeCommand({
        agent: 'codex',
        sessionId: 'session-1',
        cwd: 'C:\\Users\\Ada Lovelace\\repo',
        platform: 'win32'
      })
    ).toBe('cmd /d /s /c "cd /d ""C:\\Users\\Ada Lovelace\\repo"" && codex resume ""session-1"""')
  })

  it('builds a direct queued command for a live cmd shell', () => {
    expect(
      buildAiVaultResumeCommand({
        agent: 'omp',
        sessionId: 'session-one',
        resumeFilePath: 'C:\\Users\\Ada Lovelace\\.omp\\sessions\\A&B session one.jsonl',
        cwd: 'C:\\Users\\Ada Lovelace\\A&B repo',
        platform: 'win32',
        shell: 'cmd'
      })
    ).toBe(
      'cd /d "C:\\Users\\Ada Lovelace\\A&B repo" && omp --resume "C:\\Users\\Ada Lovelace\\.omp\\sessions\\A&B session one.jsonl"'
    )
  })

  it('carries non-default Codex homes in copied resume commands', () => {
    expect(
      buildAiVaultResumeCommand({
        agent: 'codex',
        sessionId: 'session-1',
        cwd: '/repo/app',
        platform: 'darwin',
        codexHome: '/Users/ada/Library/Application Support/Orca/codex-runtime-home/home'
      })
    ).toBe(
      "cd '/repo/app' && CODEX_HOME='/Users/ada/Library/Application Support/Orca/codex-runtime-home/home' codex resume 'session-1'"
    )

    expect(
      buildAiVaultResumeCommand({
        agent: 'codex',
        sessionId: 'session-1',
        cwd: 'C:\\Users\\Ada Lovelace\\repo',
        platform: 'win32',
        codexHome: 'C:\\Users\\Ada\\AppData\\Roaming\\Orca\\codex-runtime-home\\home'
      })
    ).toBe(
      'cmd /d /s /c "cd /d ""C:\\Users\\Ada Lovelace\\repo"" && set ""CODEX_HOME=C:\\Users\\Ada\\AppData\\Roaming\\Orca\\codex-runtime-home\\home"" && codex resume ""session-1"""'
    )
  })

  it('resumes OMP by absolute transcript path so it resolves across session-dir roots', () => {
    expect(
      buildAiVaultResumeCommand({
        agent: 'omp',
        sessionId: '019f27cd-4268-7000-96e7-62f42a55c144',
        resumeFilePath:
          '/Users/ada/.omp/agent/sessions/repo/2026-07-03T11-30-29-357Z_019f27be/OmpScannerTests.jsonl',
        cwd: '/Users/ada/repo',
        platform: 'darwin'
      })
    ).toBe(
      "cd '/Users/ada/repo' && omp --resume '/Users/ada/.omp/agent/sessions/repo/2026-07-03T11-30-29-357Z_019f27be/OmpScannerTests.jsonl'"
    )
  })

  it('quotes queued OMP resume paths for the provided Windows shell', () => {
    expect(
      buildAiVaultResumeCommand({
        agent: 'omp',
        sessionId: '019f27cd-4268-7000-96e7-62f42a55c144',
        resumeFilePath: 'C:\\Users\\Ada Lovelace\\.omp\\agent\\sessions\\repo\\sess.jsonl',
        cwd: 'C:\\Users\\Ada Lovelace\\repo',
        platform: 'win32',
        shell: 'powershell'
      })
    ).toBe(
      "Set-Location -LiteralPath 'C:\\Users\\Ada Lovelace\\repo'; omp --resume 'C:\\Users\\Ada Lovelace\\.omp\\agent\\sessions\\repo\\sess.jsonl'"
    )
  })

  it('builds nu-syntax queued resume commands for a live Nushell terminal (#8928 PR4)', () => {
    // Windows nu: nu-escaped backslash paths, `;` chaining, no `&&`/Set-Location.
    expect(
      buildAiVaultResumeCommand({
        agent: 'codex',
        sessionId: 'session-1',
        cwd: 'C:\\Users\\Ada Lovelace\\repo',
        platform: 'win32',
        codexHome: 'C:\\Users\\Ada Lovelace\\codex-home',
        shell: 'nushell'
      })
    ).toBe(
      'cd "C:\\\\Users\\\\Ada Lovelace\\\\repo"; $env.CODEX_HOME = "C:\\\\Users\\\\Ada Lovelace\\\\codex-home"; codex resume "session-1"'
    )
    // POSIX nu takes the same nu dialect — the `cd '…' && …` fallback does not parse in nu.
    expect(
      buildAiVaultResumeCommand({
        agent: 'codex',
        sessionId: 'session-1',
        cwd: '/repo/app',
        platform: 'darwin',
        shell: 'nushell'
      })
    ).toBe('cd "/repo/app"; codex resume "session-1"')
  })

  it('falls back to the session id when no OMP transcript path is known', () => {
    expect(
      buildAiVaultResumeCommand({
        agent: 'omp',
        sessionId: '019f27cd-4268-7000-96e7-62f42a55c144',
        resumeFilePath: null,
        cwd: '/Users/ada/repo',
        platform: 'darwin'
      })
    ).toBe("cd '/Users/ada/repo' && omp --resume '019f27cd-4268-7000-96e7-62f42a55c144'")
  })

  it('resumes pi by absolute transcript path since bare session ids are not resumable', () => {
    expect(
      buildAiVaultResumeCommand({
        agent: 'pi',
        sessionId: '019f27cd-4268-7000-96e7-62f42a55c144',
        resumeFilePath: '/Users/ada/.pi/agent/sessions/repo/2026-07-03T11-30-29-357Z.jsonl',
        cwd: '/Users/ada/repo',
        platform: 'darwin'
      })
    ).toBe(
      "cd '/Users/ada/repo' && pi --session '/Users/ada/.pi/agent/sessions/repo/2026-07-03T11-30-29-357Z.jsonl'"
    )
  })

  it('falls back to the session id when no pi transcript path is known', () => {
    expect(
      buildAiVaultResumeCommand({
        agent: 'pi',
        sessionId: '019f27cd-4268-7000-96e7-62f42a55c144',
        resumeFilePath: null,
        cwd: '/Users/ada/repo',
        platform: 'darwin'
      })
    ).toBe("cd '/Users/ada/repo' && pi --session '019f27cd-4268-7000-96e7-62f42a55c144'")
  })
})
