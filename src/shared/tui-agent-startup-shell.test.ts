import { describe, expect, it } from 'vitest'
import {
  buildShellCommandFromArgv,
  clearEnvCommand,
  commandSeparator,
  quoteStartupArg,
  resolveStartupShell,
  tokenizeStartupCommand
} from './tui-agent-startup-shell'

describe('nushell agent-startup dialect (#8928 PR4)', () => {
  it('quotes with nu double quotes, escaping backslashes and double quotes', () => {
    expect(quoteStartupArg('say "hi" C:\\repo', 'nushell')).toBe('"say \\"hi\\" C:\\\\repo"')
    // Why: plain nu "…" does not interpolate $ — no extra escaping.
    expect(quoteStartupArg('$env.FOO', 'nushell')).toBe('"$env.FOO"')
    expect(quoteStartupArg("fix Bob's branch", 'nushell')).toBe('"fix Bob\'s branch"')
  })

  it('carries the external caret on argv commands', () => {
    expect(buildShellCommandFromArgv(['claude', '-p', 'fix it'], 'nushell')).toBe(
      '^"claude" "-p" "fix it"'
    )
    // Empty argv stays empty — no dangling caret.
    expect(buildShellCommandFromArgv([], 'nushell')).toBe('')
  })

  it('clears env vars with hide-env -i and chains with a semicolon', () => {
    expect(clearEnvCommand('ORCA_FOO', 'nushell')).toBe('hide-env -i ORCA_FOO')
    expect(commandSeparator('nushell')).toBe('; ')
  })

  it('tokenizes nushell templates through the POSIX tokenizer', () => {
    expect(tokenizeStartupCommand(`claude 'a b' "c"`, 'nushell')).toEqual(
      tokenizeStartupCommand(`claude 'a b' "c"`, 'posix')
    )
  })

  it('keeps the platform defaults — nushell is only ever explicit', () => {
    expect(resolveStartupShell('darwin')).toBe('posix')
    expect(resolveStartupShell('win32')).toBe('powershell')
    expect(resolveStartupShell('darwin', 'nushell')).toBe('nushell')
  })
})
