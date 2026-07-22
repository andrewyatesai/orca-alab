import { execFileSync } from 'node:child_process'
import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import {
  buildWslInteractiveLoginShellCommand,
  buildWslLoginShellCommand,
  escapeWslShCommandForWindows,
  quotePosixShell
} from './wsl-login-shell-command'

const WSL_TEST_COMMAND_TIMEOUT_MS = 10_000
let wslShAvailable: boolean | null = null

function canRunWslSh(): boolean {
  if (process.platform !== 'win32') {
    return false
  }
  if (wslShAvailable !== null) {
    return wslShAvailable
  }
  try {
    execFileSync('wsl.exe', ['--', 'sh', '-lc', 'true'], {
      timeout: WSL_TEST_COMMAND_TIMEOUT_MS
    })
    wslShAvailable = true
  } catch {
    wslShAvailable = false
  }
  return wslShAvailable
}

function expectValidShSyntax(command: string): void {
  try {
    execFileSync('sh', ['-n'], { input: command, timeout: WSL_TEST_COMMAND_TIMEOUT_MS })
    return
  } catch (error) {
    if (
      process.platform !== 'win32' ||
      !(error instanceof Error && 'code' in error && error.code === 'ENOENT')
    ) {
      throw error
    }
  }
  if (!canRunWslSh()) {
    return
  }
  execFileSync('wsl.exe', ['--', 'sh', '-n'], {
    input: command,
    timeout: WSL_TEST_COMMAND_TIMEOUT_MS
  })
}

describe('wsl login shell command helpers', () => {
  it('quotes single quotes for POSIX shell arguments', () => {
    expect(quotePosixShell("a'b")).toBe("'a'\\''b'")
  })

  it('runs commands through the distro user login shell', () => {
    const command = buildWslLoginShellCommand("printf 'hello'")

    expect(command).toContain('getent passwd')
    expect(command).toContain('bash|zsh|ksh|mksh|ash)')
    expect(command).toContain('exec "$_orca_wsl_shell" -ilc')
    expect(command).toContain('exec /bin/sh -lc')
    expect(command).toContain("printf '\\''hello'\\''")
  })

  it.skipIf(process.platform === 'win32')(
    'resolves env-node launchers from the current login-shell PATH on every run',
    () => {
      const root = mkdtempSync(join(tmpdir(), 'orca-wsl-login-codex-'))
      const tools = join(root, 'tools')
      const loginBin = join(root, 'login')
      const v1Bin = join(root, 'nvm-v1')
      const v2Bin = join(root, 'nvm-v2')
      mkdirSync(tools)
      mkdirSync(loginBin)
      mkdirSync(v1Bin)
      mkdirSync(v2Bin)
      const loginShell = join(loginBin, 'bash')
      writeFileSync(
        join(tools, 'getent'),
        `#!/bin/sh\nprintf '%s\\n' "user:x:1000:1000::/home/user:$ORCA_TEST_LOGIN_SHELL"\n`
      )
      writeFileSync(
        loginShell,
        '#!/bin/sh\nexport PATH="$ORCA_TEST_CODEX_BIN:/usr/bin:/bin"\nexec /bin/sh -c "$2"\n'
      )
      for (const [bin, label] of [
        [v1Bin, 'v1'],
        [v2Bin, 'v2']
      ] as const) {
        writeFileSync(join(bin, 'codex'), '#!/usr/bin/env node\n')
        writeFileSync(join(bin, 'node'), `#!/bin/sh\nprintf '%s' '${label}'\n`)
        chmodSync(join(bin, 'codex'), 0o755)
        chmodSync(join(bin, 'node'), 0o755)
      }
      chmodSync(join(tools, 'getent'), 0o755)
      chmodSync(loginShell, 0o755)

      const command = buildWslLoginShellCommand('exec codex')
      const run = (codexBin: string): string =>
        execFileSync('/bin/sh', ['-c', command], {
          encoding: 'utf8',
          env: {
            ...process.env,
            PATH: `${tools}:/usr/bin:/bin`,
            ORCA_TEST_LOGIN_SHELL: loginShell,
            ORCA_TEST_CODEX_BIN: codexBin
          }
        })

      try {
        expect(run(v1Bin)).toBe('v1')
        // The old launcher remains executable; current PATH precedence wins.
        expect(run(v2Bin)).toBe('v2')
      } finally {
        rmSync(root, { recursive: true, force: true })
      }
    }
  )

  it('preserves command-scoped environment variables through the outer WSL shell', () => {
    const command = buildWslLoginShellCommand('HISTFILE=/tmp/orca-history printf "$HISTFILE"')
    const escaped = escapeWslShCommandForWindows(command)

    expect(command).toContain('\'HISTFILE=/tmp/orca-history printf "$HISTFILE"\'')
    expect(escaped).toContain('\\$_orca_wsl_shell')
    expect(escaped).toContain('\\${SHELL:-/bin/bash}')
    expect(escaped).toContain('\\$(getent passwd "\\$(id -un)"')
    expect(escaped).toContain('\\$HISTFILE')
    expectValidShSyntax(command)
  }, 30_000)

  it('does not double-escape wrapper shell variables', () => {
    const command = 'echo \\$_orca_wsl_shell "$_orca_wsl_shell"'

    expect(escapeWslShCommandForWindows(command)).toBe(
      'echo \\$_orca_wsl_shell "\\$_orca_wsl_shell"'
    )
  })

  it('escapes user command dollars inside POSIX-quoted payloads for WSL argv', () => {
    const command = buildWslLoginShellCommand(
      'HISTFILE=/tmp/orca-history printf "$HISTFILE"; printf \'%s\' "$SHELL"'
    )
    const escaped = escapeWslShCommandForWindows(command)

    expect(escaped).toContain(
      "'HISTFILE=/tmp/orca-history printf \"\\$HISTFILE\"; printf '\\''%s'\\'' \"\\$SHELL\"'"
    )
    expectValidShSyntax(command)
  }, 30_000)

  it('preserves user command variables across the Windows-to-WSL argv boundary', () => {
    if (!canRunWslSh()) {
      return
    }

    const command = buildWslLoginShellCommand('orca_value=ok; printf "<%s>" "$orca_value"')
    const escaped = escapeWslShCommandForWindows(command)

    expect(
      execFileSync('wsl.exe', ['--', 'sh', '-lc', escaped], {
        encoding: 'utf8',
        timeout: WSL_TEST_COMMAND_TIMEOUT_MS
      })
    ).toBe('<ok>')
  }, 30_000)

  it('starts an interactive login shell without assuming bash', () => {
    const command = buildWslInteractiveLoginShellCommand()

    expect(command).toContain('getent passwd')
    expect(command).toContain('if [ -z "$_orca_wsl_shell" ] || [ ! -x "$_orca_wsl_shell" ]; then')
    expect(command).toContain('_orca_shell_ready_root=""')
    expect(command).toContain('if [ -n "${ORCA_USER_DATA_PATH:-}" ]; then')
    expect(command).toContain('_orca_wsl_shell_name=$(basename "$_orca_wsl_shell"')
    expect(command).toContain('bash)')
    expect(command).toContain('--rcfile "${_orca_shell_ready_root}/bash/rcfile"')
    expect(command).toContain('zsh)')
    expect(command).toContain('export ZDOTDIR="${_orca_shell_ready_root}/zsh"')
    expect(command).toContain('exec "$_orca_wsl_shell" -l')
    expectValidShSyntax(command)
  })

  it('keeps unknown shells on the /bin/sh command path', () => {
    // Why: nu cannot parse POSIX command payloads; the `*)` fallback dialect must stay /bin/sh (#8928 §7).
    const command = buildWslLoginShellCommand("printf 'hello'")
    expect(command).toContain('*) exec /bin/sh -lc')
    expect(command).not.toContain('nu)')
  })

  describe('nushell interactive login (#8928 §7)', () => {
    const buildNuFixture = (versionLine: string): { root: string; env: NodeJS.ProcessEnv } => {
      const root = mkdtempSync(join(tmpdir(), 'orca-wsl-nu-'))
      const tools = join(root, 'tools')
      const userData = join(root, 'user-data')
      mkdirSync(tools)
      mkdirSync(join(userData, 'shell-ready', 'nu'), { recursive: true })
      writeFileSync(join(userData, 'shell-ready', 'nu', 'integration.nu'), '# stub integration\n')
      const nuShell = join(tools, 'nu')
      // Why: the fake nu prints its argv so the test can assert the exec'd launch shape without a real nu install.
      writeFileSync(
        join(tools, 'getent'),
        `#!/bin/sh\nprintf '%s\\n' "user:x:1000:1000::/home/user:${nuShell}"\n`
      )
      writeFileSync(
        nuShell,
        `#!/bin/sh\nif [ "$1" = "--version" ]; then printf '%s\\n' "${versionLine}"; exit 0; fi\nprintf 'NU_ARGS:%s\\n' "$*"\n`
      )
      chmodSync(join(tools, 'getent'), 0o755)
      chmodSync(nuShell, 0o755)
      return {
        root,
        env: {
          ...process.env,
          PATH: `${tools}:/usr/bin:/bin`,
          ORCA_USER_DATA_PATH: userData
        }
      }
    }

    const runInteractive = (env: NodeJS.ProcessEnv): string =>
      execFileSync('/bin/sh', ['-c', buildWslInteractiveLoginShellCommand()], {
        encoding: 'utf8',
        env
      })

    it.skipIf(process.platform === 'win32')(
      'sources the integration with split -l -e when the in-distro version gate passes',
      () => {
        const { root, env } = buildNuFixture('0.104.0')
        try {
          const output = runInteractive(env)
          expect(output).toContain(
            `NU_ARGS:-l -e source "${env.ORCA_USER_DATA_PATH}/shell-ready/nu/integration.nu"`
          )
        } finally {
          rmSync(root, { recursive: true, force: true })
        }
      }
    )

    it.skipIf(process.platform === 'win32')(
      'strips trailing version-line noise before the compare',
      () => {
        // Why: a future "0.104.0 (abc)"-style line must not silently fail the gate (Critic note 3).
        const { root, env } = buildNuFixture('0.104.0 (rev abc123)')
        try {
          expect(runInteractive(env)).toContain('NU_ARGS:-l -e source "')
        } finally {
          rmSync(root, { recursive: true, force: true })
        }
      }
    )

    it.skipIf(process.platform === 'win32')('falls back to plain nu -l below the floor', () => {
      const { root, env } = buildNuFixture('0.95.0')
      try {
        expect(runInteractive(env)).toContain('NU_ARGS:-l\n')
      } finally {
        rmSync(root, { recursive: true, force: true })
      }
    })

    it.skipIf(process.platform === 'win32')(
      'falls back to plain nu -l when the version probe emits garbage',
      () => {
        const { root, env } = buildNuFixture('nu, version unknown')
        try {
          expect(runInteractive(env)).toContain('NU_ARGS:-l\n')
        } finally {
          rmSync(root, { recursive: true, force: true })
        }
      }
    )

    it.skipIf(process.platform === 'win32')(
      'falls back to plain nu -l when the integration file is missing',
      () => {
        const { root, env } = buildNuFixture('0.104.0')
        try {
          rmSync(join(env.ORCA_USER_DATA_PATH as string, 'shell-ready'), {
            recursive: true,
            force: true
          })
          expect(runInteractive(env)).toContain('NU_ARGS:-l\n')
        } finally {
          rmSync(root, { recursive: true, force: true })
        }
      }
    )

    it('keeps the nu case inside valid sh syntax after Windows dollar escaping', () => {
      const command = buildWslInteractiveLoginShellCommand()
      expect(command).toContain('nu)')
      expect(command).toContain('sort -V')
      expectValidShSyntax(command)
      expectValidShSyntax(escapeWslShCommandForWindows(command).replaceAll('\\$', '$'))
    })
  })
})
