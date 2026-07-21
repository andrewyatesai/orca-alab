import { describe, expect, it } from 'vitest'
import { buildWslBridgeScript, buildWslLauncher, getWslBridgeMarker } from './wsl-cli-scripts'

describe('buildWslBridgeScript', () => {
  const script = buildWslBridgeScript()

  it('dedupes case-colliding env names before invoking the launcher (#9498)', () => {
    expect(script).toContain('Repair-OrcaDuplicateEnvNames')
    expect(script).toContain('GetEnvironmentStringsW')
    expect(script).toContain('FreeEnvironmentStringsW')
    // Why: the dedupe must run before the launcher spawn or .NET children still crash.
    expect(script.indexOf('Repair-OrcaDuplicateEnvNames')).toBeLessThan(
      script.indexOf('& $OrcaLauncher @ForwardArgs')
    )
  })

  it('keeps the managed marker and launcher contract intact', () => {
    expect(script.startsWith(getWslBridgeMarker())).toBe(true)
    expect(script).toContain('& $OrcaLauncher @ForwardArgs')
    expect(script).toContain('exit $exitCode')
    // Why: a stray TS template interpolation would serialize as 'undefined' in the script.
    expect(script).not.toContain('undefined')
  })

  it('stays embeddable in the installer bash heredoc', () => {
    // Why: the installer writes the bridge via <<'ORCA_WSL_BRIDGE'; a line equal
    // to the terminator would truncate the script.
    expect(script.split('\n').some((line) => line.trim() === 'ORCA_WSL_BRIDGE')).toBe(false)
    // Why: PowerShell here-string terminators must sit at line starts to parse.
    expect(script).toContain("$definition = @'\n")
    expect(/^'@$/m.test(script)).toBe(true)
  })
})

describe('buildWslLauncher', () => {
  it('pins Windows PowerShell for the bridge so the Framework env-dup detector applies', () => {
    const launcher = buildWslLauncher('C:\\Users\\alice\\AppData\\Local\\Orca\\orca.cmd')
    expect(launcher).toContain('powershell.exe')
    expect(launcher).toContain('-File "$ORCA_BRIDGE_PS1_WIN"')
  })
})
