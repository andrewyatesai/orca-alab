import { describe, expect, it } from 'vitest'
import {
  encodePowerShellCommand,
  getPowerShellOsc133Bootstrap
} from './powershell-osc133-bootstrap'

describe('PowerShell OSC 133 bootstrap', () => {
  it('wraps prompt/readline without bypassing profiles or execution policy', () => {
    const script = getPowerShellOsc133Bootstrap()

    expect(script).toContain('[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()')
    expect(script).toContain('ORCA_OPENCODE_CONFIG_DIR')
    expect(script).toContain('ORCA_MIMOCODE_HOME')
    expect(script).not.toContain('ORCA_PI_CODING_AGENT_DIR')
    expect(script).not.toContain('ORCA_OMP_CODING_AGENT_DIR')
    expect(script).toContain('ORCA_OMP_STATUS_EXTENSION')
    expect(script).toContain('function Global:omp')
    expect(script).toContain('--extension $env:ORCA_OMP_STATUS_EXTENSION')
    expect(script).toContain('ORCA_CODEX_HOME')
    expect(script).toContain('function Global:prompt')
    expect(script).toContain('function Global:PSConsoleHostReadLine')
    expect(script).toContain('Esc = [char]27')
    expect(script).toContain('Bel = [char]7')
    expect(script).toContain(')]133;D;$fakeExitCode$(')
    expect(script).toContain(')]133;A$(')
    expect(script).toContain(')]133;B$(')
    expect(script).toContain(')]133;C$(')
    expect(script).not.toContain('`e]133')
    expect(script).not.toContain('$PROFILE')
    expect(script).not.toContain('ExecutionPolicy')
    expect(script).not.toContain('NoProfile')
  })

  it('encodes commands as UTF-16LE base64 for PowerShell -EncodedCommand', () => {
    expect(encodePowerShellCommand('Write-Output ok')).toBe(
      Buffer.from('Write-Output ok', 'utf16le').toString('base64')
    )
  })

  // Why: #7596 — the readline hook must emit the escaped command line as 633;E
  // before 133;C so cold restore can offer a re-run of the last command.
  it('emits escaped OSC 633;E before 133;C in the readline hook', () => {
    const script = getPowerShellOsc133Bootstrap()

    expect(script).toContain(')]633;E;$orcaCl$(')
    // VS Code escaping: \ → \\, ; → \x3b, CRLF/LF/CR → \x0a; 2 KB truncation.
    expect(script).toContain(".Replace('\\', '\\\\').Replace(';', '\\x3b')")
    expect(script).toContain(".Replace(\"`r`n\", '\\x0a').Replace(\"`n\", '\\x0a').Replace(\"`r\", '\\x0a')")
    expect(script).toContain('$orcaCl.Substring(0, 2048)')
    const readlineHook = script.slice(script.indexOf('function Global:PSConsoleHostReadLine'))
    expect(readlineHook.indexOf(']633;E;')).toBeLessThan(readlineHook.indexOf(']133;C$('))
  })
})
