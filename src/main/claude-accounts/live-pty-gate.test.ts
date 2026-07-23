import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  attachClaudeLivePtyDrainListener,
  attachClaudeLivePtyPersistence,
  beginClaudeAuthSwitch,
  confirmSeededClaudeLivePtys,
  endClaudeAuthSwitch,
  hasLiveClaudePtys,
  isClaudeAuthSwitchInProgress,
  markClaudePtyExited,
  markClaudePtySpawned,
  seedLiveClaudePtysFromPersistence
} from './live-pty-gate'

describe('Claude live PTY gate', () => {
  afterEach(() => {
    markClaudePtyExited('live-claude-pty')
    markClaudePtyExited('seeded-pty-1')
    markClaudePtyExited('seeded-pty-2')
    confirmSeededClaudeLivePtys([])
    attachClaudeLivePtyPersistence(null)
    attachClaudeLivePtyDrainListener(null)
    endClaudeAuthSwitch()
  })

  it('allows switching while Claude PTYs are live', () => {
    markClaudePtySpawned('live-claude-pty')

    beginClaudeAuthSwitch()

    expect(isClaudeAuthSwitchInProgress()).toBe(true)
  })

  it('still rejects overlapping account switches', () => {
    beginClaudeAuthSwitch()

    expect(() => beginClaudeAuthSwitch()).toThrow('already in progress')
  })

  it('counts seeded session ids as live until confirmed dead', () => {
    seedLiveClaudePtysFromPersistence(['seeded-pty-1', 'seeded-pty-2'])

    expect(hasLiveClaudePtys()).toBe(true)

    confirmSeededClaudeLivePtys(['seeded-pty-1'])

    expect(hasLiveClaudePtys()).toBe(true)

    confirmSeededClaudeLivePtys([])

    expect(hasLiveClaudePtys()).toBe(true)

    markClaudePtyExited('seeded-pty-1')

    expect(hasLiveClaudePtys()).toBe(false)
  })

  it('releases seeded ids the daemon no longer knows', () => {
    const removeClaudeLivePtySessionId = vi.fn()
    attachClaudeLivePtyPersistence({
      addClaudeLivePtySessionId: vi.fn(),
      removeClaudeLivePtySessionId
    })
    seedLiveClaudePtysFromPersistence(['seeded-pty-1', 'seeded-pty-2'])

    confirmSeededClaudeLivePtys(['seeded-pty-2'])

    expect(hasLiveClaudePtys()).toBe(true)
    expect(removeClaudeLivePtySessionId).toHaveBeenCalledWith('seeded-pty-1')
    expect(removeClaudeLivePtySessionId).not.toHaveBeenCalledWith('seeded-pty-2')
  })

  it('keeps a seeded id confirmed by a real spawn out of later pruning', () => {
    seedLiveClaudePtysFromPersistence(['seeded-pty-1'])
    markClaudePtySpawned('seeded-pty-1')

    confirmSeededClaudeLivePtys([])

    expect(hasLiveClaudePtys()).toBe(true)
  })

  it('persists spawns and exits when persistence is attached', () => {
    const addClaudeLivePtySessionId = vi.fn()
    const removeClaudeLivePtySessionId = vi.fn()
    attachClaudeLivePtyPersistence({
      addClaudeLivePtySessionId,
      removeClaudeLivePtySessionId
    })

    markClaudePtySpawned('live-claude-pty')
    expect(addClaudeLivePtySessionId).toHaveBeenCalledWith('live-claude-pty')

    markClaudePtyExited('live-claude-pty')
    expect(removeClaudeLivePtySessionId).toHaveBeenCalledWith('live-claude-pty')
  })

  it('drains once when the last live Claude PTY exits, not on a non-last exit', () => {
    const onDrain = vi.fn()
    attachClaudeLivePtyDrainListener(onDrain)

    markClaudePtySpawned('live-claude-pty')
    markClaudePtySpawned('seeded-pty-1')

    // A non-last exit still leaves one live PTY — no drain.
    markClaudePtyExited('live-claude-pty')
    expect(hasLiveClaudePtys()).toBe(true)
    expect(onDrain).not.toHaveBeenCalled()

    // The last exit crosses the 1→0 transition — drain exactly once.
    markClaudePtyExited('seeded-pty-1')
    expect(hasLiveClaudePtys()).toBe(false)
    expect(onDrain).toHaveBeenCalledTimes(1)

    // Exiting an already-empty gate does not re-fire the drain.
    markClaudePtyExited('seeded-pty-1')
    expect(onDrain).toHaveBeenCalledTimes(1)
  })

  it('drains when confirming seeded ids empties the live gate', () => {
    const onDrain = vi.fn()
    attachClaudeLivePtyDrainListener(onDrain)

    seedLiveClaudePtysFromPersistence(['seeded-pty-1', 'seeded-pty-2'])

    // Pruning one dead seeded id still leaves one live — no drain.
    confirmSeededClaudeLivePtys(['seeded-pty-1'])
    expect(hasLiveClaudePtys()).toBe(true)
    expect(onDrain).not.toHaveBeenCalled()

    // Marking the surviving seeded id dead crosses 1→0 — drain once.
    markClaudePtyExited('seeded-pty-1')
    expect(hasLiveClaudePtys()).toBe(false)
    expect(onDrain).toHaveBeenCalledTimes(1)
  })
})
