import { describe, expect, it } from 'vitest'
import {
  describeOrcaDeepLinkForLog,
  MAX_ORCA_DEEP_LINK_LENGTH,
  parseOrcaDeepLink
} from './orca-deep-link'

describe('parseOrcaDeepLink', () => {
  it('parses focus handle', () => {
    expect(parseOrcaDeepLink('orca://focus/term_9c1f4c2a-2f57-4f6e-9a3e-0c1b2d3e4f5a')).toEqual({
      kind: 'focus',
      handle: 'term_9c1f4c2a-2f57-4f6e-9a3e-0c1b2d3e4f5a'
    })
  })

  it('accepts case-insensitive scheme and host', () => {
    expect(parseOrcaDeepLink('ORCA://FOCUS/term_abc')).toEqual({
      kind: 'focus',
      handle: 'term_abc'
    })
  })

  it('rejects focus with path traversal segments', () => {
    expect(parseOrcaDeepLink('orca://focus/term_abc/../term_def')).toBeNull()
    expect(parseOrcaDeepLink('orca://focus/term_abc/extra')).toBeNull()
    expect(parseOrcaDeepLink('orca://focus/..%2Fterm_abc')).toBeNull()
  })

  it('rejects non-term handles', () => {
    expect(parseOrcaDeepLink('orca://focus/task_123')).toBeNull()
    expect(parseOrcaDeepLink('orca://focus/term_')).toBeNull()
    expect(parseOrcaDeepLink('orca://focus/term_%20abc')).toBeNull()
    expect(parseOrcaDeepLink(`orca://focus/term_${'a'.repeat(129)}`)).toBeNull()
  })

  it('round-trips worktree ids containing :: and /', () => {
    const worktreeId = 'repo-1::/Users/dev/src/my repo/worktrees/feature'
    const parsed = parseOrcaDeepLink(`orca://worktree/${encodeURIComponent(worktreeId)}`)
    expect(parsed).toEqual({ kind: 'worktree', worktreeId })
  })

  it('parses worktree tab query', () => {
    const parsed = parseOrcaDeepLink(`orca://worktree/${encodeURIComponent('r::p')}?tab=tab-9`)
    expect(parsed).toEqual({ kind: 'worktree', worktreeId: 'r::p', tabId: 'tab-9' })
  })

  it('rejects oversized urls', () => {
    const raw = `orca://focus/term_${'a'.repeat(MAX_ORCA_DEEP_LINK_LENGTH)}`
    expect(parseOrcaDeepLink(raw)).toBeNull()
  })

  it('rejects credentialed urls', () => {
    expect(parseOrcaDeepLink('orca://user:pass@focus/term_abc')).toBeNull()
    expect(parseOrcaDeepLink('orca://user@focus/term_abc')).toBeNull()
  })

  it('rejects unknown hosts (orca://pairing precedent)', () => {
    expect(parseOrcaDeepLink('orca://pairing?code=abc')).toBeNull()
    expect(parseOrcaDeepLink('orca://settings')).toBeNull()
    expect(parseOrcaDeepLink('orca://')).toBeNull()
    expect(parseOrcaDeepLink('orca:focus/term_abc')).toBeNull()
  })

  it('rejects non-orca schemes and unparseable urls', () => {
    expect(parseOrcaDeepLink('https://focus/term_abc')).toBeNull()
    expect(parseOrcaDeepLink('not a url')).toBeNull()
    expect(parseOrcaDeepLink('')).toBeNull()
  })

  it('parses pair code from query or fragment', () => {
    expect(parseOrcaDeepLink('orca://pair?code=YWJj')).toEqual({ kind: 'pair', code: 'YWJj' })
    expect(parseOrcaDeepLink('orca://pair#YWJj')).toEqual({ kind: 'pair', code: 'YWJj' })
    expect(parseOrcaDeepLink('orca://pair')).toBeNull()
    expect(parseOrcaDeepLink('orca://pair/extra?code=YWJj')).toBeNull()
  })

  it('run requires worktree and cmd', () => {
    expect(parseOrcaDeepLink('orca://run?worktree=r%3A%3Ap')).toBeNull()
    expect(parseOrcaDeepLink('orca://run?cmd=ls')).toBeNull()
    expect(parseOrcaDeepLink('orca://run?worktree=r%3A%3Ap&cmd=')).toBeNull()
    expect(parseOrcaDeepLink('orca://run?worktree=r%3A%3Ap&cmd=ls%20-la&title=List')).toEqual({
      kind: 'run',
      worktreeId: 'r::p',
      command: 'ls -la',
      title: 'List'
    })
  })
})

describe('describeOrcaDeepLinkForLog', () => {
  it('redacts pair codes but keeps navigation targets', () => {
    expect(describeOrcaDeepLinkForLog({ kind: 'pair', code: 'secret' })).not.toContain('secret')
    expect(describeOrcaDeepLinkForLog({ kind: 'focus', handle: 'term_a' })).toContain('term_a')
  })

  it('never exposes the run command (may embed tokens)', () => {
    const described = describeOrcaDeepLinkForLog({
      kind: 'run',
      worktreeId: 'r::p',
      command: 'deploy --token=abc'
    })
    expect(described).not.toContain('abc')
  })
})
