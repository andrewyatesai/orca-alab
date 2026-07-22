import { describe, expect, it } from 'vitest'
import { isShellProcess } from './shell-process-detection'

describe('isShellProcess', () => {
  it('recognizes the classic shells by bare name and path', () => {
    expect(isShellProcess('zsh')).toBe(true)
    expect(isShellProcess('/bin/bash')).toBe(true)
    expect(isShellProcess('fish')).toBe(true)
    expect(isShellProcess('pwsh.exe')).toBe(true)
  })

  // Why: regression pin for #8928 — nu was already listed; agent-status fallback depends on it staying listed.
  it('recognizes nu as a shell (bare, path, and Windows extension forms)', () => {
    expect(isShellProcess('nu')).toBe(true)
    expect(isShellProcess('/usr/local/bin/nu')).toBe(true)
    expect(isShellProcess('nu.exe')).toBe(true)
  })

  it('does not classify agent CLIs as shells', () => {
    expect(isShellProcess('claude')).toBe(false)
    expect(isShellProcess('codex')).toBe(false)
    expect(isShellProcess('node')).toBe(false)
  })
})
