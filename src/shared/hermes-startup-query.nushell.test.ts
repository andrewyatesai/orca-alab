import { execFileSync } from 'node:child_process'
import { describe, expect, it } from 'vitest'
import { planHermesStartupQuery } from './hermes-startup-query'

// Routing contract for the nushell startup family (#8928 PR4): a POSIX-host nu
// terminal gets the `sh -c` wrapper (nu parses the plain single-quoted arg and
// `sh` runs the POSIX grammar); a win32 nu terminal has no `sh`, so it keeps
// the PowerShell -EncodedCommand wrapper whose tokens are all nu bare words.
describe('planHermesStartupQuery nushell routing', () => {
  const baseArgs = {
    baseCommand: 'hermes --tui',
    prompt: 'find the bug',
    platform: 'darwin' as NodeJS.Platform,
    shell: 'nushell' as const
  }

  it('routes POSIX-host nu terminals through the sh wrapper', () => {
    const plan = planHermesStartupQuery(baseArgs)
    expect(plan?.command.startsWith('sh -c ')).toBe(true)
    // Why: nu single-quoted strings have no escape mechanism, so the wrapper arg must contain no quotes beyond the outer pair.
    const wrapperArg = plan?.command.slice('sh -c '.length) ?? ''
    expect(wrapperArg.startsWith("'")).toBe(true)
    expect(wrapperArg.endsWith("'")).toBe(true)
    expect(wrapperArg.slice(1, -1)).not.toContain("'")
  })

  it('keeps win32 nu terminals on the PowerShell wrapper', () => {
    const plan = planHermesStartupQuery({ ...baseArgs, platform: 'win32' })
    expect(plan?.command.startsWith('powershell.exe -NoProfile -EncodedCommand ')).toBe(true)
  })

  it('allows env-assignment prefixes only where the sh wrapper runs', () => {
    const withAssignment = { ...baseArgs, baseCommand: 'HERMES_MODE=fast hermes --tui' }
    expect(planHermesStartupQuery(withAssignment)).not.toBeNull()
    expect(planHermesStartupQuery({ ...withAssignment, platform: 'win32' })).toBeNull()
  })

  it.skipIf(process.platform === 'win32')('produces a wrapper that sh actually parses', () => {
    const plan = planHermesStartupQuery(baseArgs)
    const wrapperArg = plan?.command.slice('sh -c '.length) ?? ''
    // Strip the POSIX quoting to recover the raw script, then syntax-check it.
    execFileSync('sh', ['-n'], { input: wrapperArg.slice(1, -1) })
  })
})
