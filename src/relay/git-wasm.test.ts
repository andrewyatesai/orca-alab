import { describe, expect, it } from 'vitest'
import { detectPiAgentKindFromCommand, upstreamOnlyCommitsArePatchEquivalent } from './git-wasm'

// The wrappers initSync the embedded wasm lazily — no setup needed. These pin
// the relay-side wasm path for the functions whose shared TS was deleted
// (ported from the deleted halves of the shared test files; the spy-based
// TS-internal assertions were dropped with the TS implementations).

describe('upstreamOnlyCommitsArePatchEquivalent (orca-git wasm)', () => {
  it('returns true when every upstream-only commit is patch-equivalent', () => {
    expect(upstreamOnlyCommitsArePatchEquivalent('= abc\n= def\n')).toBe(true)
  })

  it('returns false for empty output or non-equivalent commits', () => {
    expect(upstreamOnlyCommitsArePatchEquivalent('')).toBe(false)
    expect(upstreamOnlyCommitsArePatchEquivalent('= abc\n+ def\n')).toBe(false)
  })

  it('scans newline-heavy CRLF cherry output', () => {
    expect(
      upstreamOnlyCommitsArePatchEquivalent(`${'\r\n'.repeat(10_000)}= abc\r\n= def\r\n`)
    ).toBe(true)
  })
})

describe('detectPiAgentKindFromCommand (orca-git wasm)', () => {
  it('matches the napi-side detector for the boundary cases', () => {
    expect(detectPiAgentKindFromCommand(undefined)).toBe('pi')
    expect(detectPiAgentKindFromCommand('omp.sh --resume')).toBe('omp')
    expect(detectPiAgentKindFromCommand('pip install foo')).toBe('pi')
    expect(detectPiAgentKindFromCommand('pomp.exe')).toBe('pi')
  })
})
