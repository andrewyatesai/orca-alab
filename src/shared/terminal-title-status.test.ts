import { describe, expect, it } from 'vitest'

import { detectAgentStatusFromTitle } from './terminal-title-status'

describe('terminal-title-status authoritative-hook name-only defer', () => {
  // Why: Droid/Hermes/Antigravity ship authoritative hook services (#6011). This
  // sibling classifier (used by terminal-title-display) must match agent-title-status:
  // a bare name-only native title defers to the hook (null), not fall through to
  // 'idle' and race a still-working pane to false completion. Previously only Droid
  // was guarded here; Hermes/agy fell through to 'idle'.
  it.each(['Droid', 'Hermes', 'agy'])(
    'defers name-only authoritative-hook title %j to hooks (null, not idle)',
    (title) => {
      expect(detectAgentStatusFromTitle(title)).toBeNull()
    }
  )

  // Why: explicit-status titles still classify; the defer only covers name-only.
  it.each([
    ['Hermes ready', 'idle'],
    ['Hermes working', 'working'],
    ['⠋ Hermes', 'working'],
    ['Hermes - action required', 'permission'],
    ['. Hermes', 'working'],
    ['* Hermes', 'idle'],
    ['agy ready', 'idle'],
    ['⠋ agy', 'working']
  ] as const)('still classifies explicit-status hook title %j as %s', (title, expected) => {
    expect(detectAgentStatusFromTitle(title)).toBe(expected)
  })

  // Why: 'antigravity' is a legacy agent name, so a bare legacy-named title keeps
  // its idle default (the defer is scoped to non-legacy name-only titles).
  it('keeps the idle default for a bare legacy-named title', () => {
    expect(detectAgentStatusFromTitle('Antigravity')).toBe('idle')
  })

  // Why: a non-agent title still returns null (no name match at all).
  it('returns null for a title with no agent identity', () => {
    expect(detectAgentStatusFromTitle('~/projects/build')).toBeNull()
  })
})
