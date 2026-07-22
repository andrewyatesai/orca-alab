import { describe, expect, it } from 'vitest'
import { buildSshLoginShellCommand } from './ssh-login-shell-command'

describe('buildSshLoginShellCommand', () => {
  it('uses -lc for login-capable POSIX shells', () => {
    expect(buildSshLoginShellCommand('/bin/bash', 'command -v node')).toBe(
      "'/bin/bash' -lc 'command -v node'"
    )
    expect(buildSshLoginShellCommand('/usr/bin/zsh', 'command -v node')).toBe(
      "'/usr/bin/zsh' -lc 'command -v node'"
    )
    expect(buildSshLoginShellCommand('/usr/bin/fish', 'command -v node')).toBe(
      "'/usr/bin/fish' -lc 'command -v node'"
    )
  })

  it('uses bare -c for shells that reject combined -lc or need no login mode', () => {
    for (const shell of ['/bin/sh', '/bin/dash', '/bin/csh', '/bin/tcsh']) {
      expect(buildSshLoginShellCommand(shell, 'command -v node')).toBe(
        `'${shell}' -c 'command -v node'`
      )
    }
  })

  it('nu login shell gets caret-quoted head, split flags, sh-delegated probe (#7715)', () => {
    expect(buildSshLoginShellCommand('/usr/local/bin/nu', 'command -v node')).toBe(
      `^'/usr/local/bin/nu' -l -c "^sh -c 'command -v node'"`
    )
    // Versioned/cargo installs classify as nu too.
    expect(buildSshLoginShellCommand('/home/u/.cargo/bin/nu-0.104', 'command -v node')).toBe(
      `^'/home/u/.cargo/bin/nu-0.104' -l -c "^sh -c 'command -v node'"`
    )
  })

  it('falls back to the /bin/sh form when the probe text cannot be nu-quoted', () => {
    // Why: nu single-quoted strings have no escapes and nu double quotes eat backslashes.
    expect(buildSshLoginShellCommand('/usr/bin/nu', "echo 'quoted'")).toBe(
      `/bin/sh -c 'echo '\\''quoted'\\'''`
    )
    expect(buildSshLoginShellCommand('/usr/bin/nu', 'printf "%s" x')).toBe(
      `/bin/sh -c 'printf "%s" x'`
    )
    expect(buildSshLoginShellCommand("/odd'path/nu", 'command -v node')).toBe(
      `/bin/sh -c 'command -v node'`
    )
  })

  it('keeps non-nu shells with nu-like substrings on the POSIX branch', () => {
    expect(buildSshLoginShellCommand('/usr/bin/nushell-wrapper', 'command -v node')).toBe(
      "'/usr/bin/nushell-wrapper' -lc 'command -v node'"
    )
  })
})
