import { describe, expect, it } from 'vitest'
import {
  buildNuSourceCommand,
  isNushellExecutableName,
  NUSHELL_INTEGRATION_MIN_VERSION,
  quoteNuDoubleQuoted,
  WINDOWS_NUSHELL_SHELL
} from './nushell-shell'

describe('isNushellExecutableName', () => {
  it('matches bare names, .exe, and versioned basenames', () => {
    expect(isNushellExecutableName('nu')).toBe(true)
    expect(isNushellExecutableName('NU')).toBe(true)
    expect(isNushellExecutableName('nu.exe')).toBe(true)
    expect(isNushellExecutableName('nu-0.104')).toBe(true)
    expect(isNushellExecutableName('nu-0.104.1.exe')).toBe(true)
  })

  it('matches paths with either slash', () => {
    expect(isNushellExecutableName('/usr/local/bin/nu')).toBe(true)
    expect(isNushellExecutableName('/home/u/.cargo/bin/nu-0.96')).toBe(true)
    expect(isNushellExecutableName('C:\\Program Files\\nu\\bin\\nu.exe')).toBe(true)
  })

  it('rejects other shells and nu-prefixed binaries', () => {
    expect(isNushellExecutableName('bash')).toBe(false)
    expect(isNushellExecutableName('zsh')).toBe(false)
    expect(isNushellExecutableName('numbat')).toBe(false)
    // Why: 'nushell' is the settings sentinel, not an executable basename.
    expect(isNushellExecutableName('nushell')).toBe(false)
    expect(isNushellExecutableName('/usr/bin/gnu')).toBe(false)
    expect(isNushellExecutableName('')).toBe(false)
  })

  it('keeps the Windows sentinel out of executable matching', () => {
    expect(isNushellExecutableName(WINDOWS_NUSHELL_SHELL)).toBe(false)
  })
})

describe('quoteNuDoubleQuoted', () => {
  it('escapes backslashes and double quotes', () => {
    expect(quoteNuDoubleQuoted('plain')).toBe('"plain"')
    expect(quoteNuDoubleQuoted('C:\\Users\\me')).toBe('"C:\\\\Users\\\\me"')
    expect(quoteNuDoubleQuoted('say "hi"')).toBe('"say \\"hi\\""')
  })

  it('leaves $ uninterpolated-safe (plain nu double quotes do not expand $)', () => {
    expect(quoteNuDoubleQuoted('$HOME/dir')).toBe('"$HOME/dir"')
  })
})

describe('buildNuSourceCommand', () => {
  it('wraps the integration path in a nu double-quoted source command', () => {
    expect(buildNuSourceCommand('/data/shell-ready/nu/integration.nu')).toBe(
      'source "/data/shell-ready/nu/integration.nu"'
    )
  })

  // Why: adversarial — a userData path containing quotes/backslashes must not break out of the -e string.
  it('escapes hostile userData paths', () => {
    expect(buildNuSourceCommand('/tmp/o"dd\\dir/integration.nu')).toBe(
      'source "/tmp/o\\"dd\\\\dir/integration.nu"'
    )
  })
})

describe('NUSHELL_INTEGRATION_MIN_VERSION', () => {
  it('pins the 0.96.0 integration floor from the design', () => {
    expect(NUSHELL_INTEGRATION_MIN_VERSION).toBe('0.96.0')
  })
})
