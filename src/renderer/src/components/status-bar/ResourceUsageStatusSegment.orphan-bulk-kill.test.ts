import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

const SOURCE_PATH = resolve(__dirname, 'ResourceUsageStatusSegment.tsx')

// Issue #8459: the bulk "Kill orphan terminals" action force-killed live daemon
// sessions using the popover's stale session snapshot, with no confirmation.
describe('ResourceUsageStatusSegment orphan bulk kill', () => {
  const source = readFileSync(SOURCE_PATH, 'utf8')

  it('routes the bulk kill button through a confirm dialog, never a direct kill', () => {
    expect(source).toContain('onClick={() => setOrphanKillConfirm(true)}')
    expect(source).not.toContain('handleKillOrphans')
  })

  it('re-verifies orphans against fresh daemon inventory before killing', () => {
    const confirmedIndex = source.indexOf('const runKillOrphansConfirmed')
    expect(confirmedIndex).toBeGreaterThanOrEqual(0)
    const refreshIndex = source.indexOf('await refreshSessions()', confirmedIndex)
    const verifyIndex = source.indexOf('selectVerifiedOrphanSessions(', confirmedIndex)
    const killIndex = source.indexOf('window.api.pty.kill', confirmedIndex)
    // Order matters: fetch fresh inventory -> re-verify unbound -> kill.
    expect(refreshIndex).toBeGreaterThan(confirmedIndex)
    expect(verifyIndex).toBeGreaterThan(refreshIndex)
    expect(killIndex).toBeGreaterThan(verifyIndex)
  })

  it('aborts the bulk kill when the fresh inventory fetch fails', () => {
    const confirmedIndex = source.indexOf('const runKillOrphansConfirmed')
    const guardIndex = source.indexOf('if (freshSessions === null)', confirmedIndex)
    const killIndex = source.indexOf('window.api.pty.kill', confirmedIndex)
    expect(guardIndex).toBeGreaterThan(confirmedIndex)
    expect(killIndex).toBeGreaterThan(guardIndex)
  })
})
