import { describe, expect, it } from 'vitest'
import { buildEnvShellPrefix, resolveCustomAgentBaseCommand } from './custom-agent-profile'

describe('buildEnvShellPrefix', () => {
  it('returns empty when env is undefined or empty', () => {
    expect(buildEnvShellPrefix(undefined, 'linux')).toBe('')
    expect(buildEnvShellPrefix({}, 'linux')).toBe('')
  })

  it('quotes values with POSIX single quotes on linux/darwin', () => {
    expect(buildEnvShellPrefix({ FOO: 'bar' }, 'linux')).toBe("FOO='bar' ")
    expect(buildEnvShellPrefix({ A: 'a', B: 'b' }, 'darwin')).toBe("A='a' B='b' ")
  })

  it('escapes single quotes via close-reopen on POSIX', () => {
    // Why: this is the only way to embed a literal `'` inside a single-quoted
    // POSIX string. Regression test guards against accidental switch to a
    // simpler-but-wrong escape (e.g. backslash, which single quotes don't honor).
    expect(buildEnvShellPrefix({ T: "a'b" }, 'linux')).toBe("T='a'\\''b' ")
  })

  it('uses double-quote-doubled values on win32', () => {
    expect(buildEnvShellPrefix({ FOO: 'a"b' }, 'win32')).toBe('FOO="a""b" ')
  })

  it('drops empty keys', () => {
    expect(buildEnvShellPrefix({ '': 'x', K: 'v' }, 'linux')).toBe("K='v' ")
  })
})

describe('resolveCustomAgentBaseCommand', () => {
  it('prepends the env prefix to the user command', () => {
    expect(
      resolveCustomAgentBaseCommand(
        {
          id: 'p1',
          label: 'Claude (zai)',
          baseAgent: 'claude',
          command: 'claude --dangerously-skip-permissions',
          env: { ANTHROPIC_BASE_URL: 'http://localhost:1234' }
        },
        'linux'
      )
    ).toBe("ANTHROPIC_BASE_URL='http://localhost:1234' claude --dangerously-skip-permissions")
  })

  it('returns just the command when no env is set', () => {
    expect(
      resolveCustomAgentBaseCommand(
        {
          id: 'p1',
          label: 'Claude (default)',
          baseAgent: 'claude',
          command: 'claude'
        },
        'linux'
      )
    ).toBe('claude')
  })
})
