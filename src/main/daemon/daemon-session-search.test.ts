import { describe, expect, it, vi } from 'vitest'
import {
  buildReplaySearchContent,
  daemonSearchSupported,
  fetchDaemonSearchContext,
  fetchDeadSessionSearchContext,
  searchDaemonSessions,
  searchDeadSessionHistory,
  type DaemonSearchTransport
} from './daemon-session-search'
import { SESSION_SEARCH_PROTOCOL_VERSION } from './daemon-protocol-versions'
import type { TerminalCheckpointFile } from './daemon-checkpoint-file'
import type { TerminalHistoryLogContents } from './terminal-history-log'

function transport(
  handler: (type: string, payload: unknown) => unknown,
  protocolVersion = SESSION_SEARCH_PROTOCOL_VERSION
): DaemonSearchTransport & { calls: { type: string; payload: unknown }[] } {
  const calls: { type: string; payload: unknown }[] = []
  return {
    protocolVersion,
    calls,
    async request<T>(type: string, payload: unknown): Promise<T> {
      calls.push({ type, payload })
      return handler(type, payload) as T
    }
  }
}

const checkpoint = (overrides: Partial<TerminalCheckpointFile> = {}): TerminalCheckpointFile => ({
  snapshotAnsi: 'SNAPSHOT',
  scrollbackAnsi: 'SCROLLBACK',
  rehydrateSequences: '',
  cwd: null,
  cols: 80,
  rows: 24,
  modes: { alternateScreen: false } as TerminalCheckpointFile['modes'],
  scrollbackLines: 10,
  generation: 2,
  checkpointedAt: '2026-01-01T00:00:00.000Z',
  ...overrides
})

describe('daemonSearchSupported', () => {
  it('gates on the v1021 search protocol version', () => {
    expect(daemonSearchSupported(SESSION_SEARCH_PROTOCOL_VERSION)).toBe(true)
    expect(daemonSearchSupported(SESSION_SEARCH_PROTOCOL_VERSION - 1)).toBe(false)
  })
})

describe('searchDaemonSessions', () => {
  it('returns summaries and forwards allowlist + cutoffs', async () => {
    const t = transport((type) => {
      expect(type).toBe('searchSessions')
      return {
        sessions: [
          {
            sessionId: 's1',
            matches: [{ absRow: 4, col: 0, len: 6, line: 'needle here' }],
            total: 3,
            incomplete: true
          }
        ]
      }
    })
    const result = await searchDaemonSessions(t, {
      query: 'needle',
      sessionIds: ['s1'],
      cutoffRows: { s1: 100 },
      maxPerSession: 25,
      gen: 9
    })
    expect(result.available).toBe(true)
    expect(result.sessions).toHaveLength(1)
    expect(result.sessions[0].incomplete).toBe(true)
    expect(t.calls[0].payload).toMatchObject({
      query: 'needle',
      caseSensitive: false,
      regex: false,
      sessionIds: ['s1'],
      cutoffRows: { s1: 100 },
      maxPerSession: 25,
      gen: 9
    })
  })

  it('degrades to unavailable on an old daemon WITHOUT sending the RPC', async () => {
    const t = transport(() => {
      throw new Error('must not be called')
    }, SESSION_SEARCH_PROTOCOL_VERSION - 1)
    const result = await searchDaemonSessions(t, { query: 'x' })
    expect(result).toEqual({ available: false, sessions: [] })
    expect(t.calls).toHaveLength(0)
  })

  it('treats a runtime "unsupported request type" reply as unavailable', async () => {
    const t = transport(() => {
      throw new Error('unsupported request type: searchSessions')
    })
    const result = await searchDaemonSessions(t, { query: 'x' })
    expect(result).toEqual({ available: false, sessions: [] })
  })

  it('rethrows genuine transport failures', async () => {
    const t = transport(() => {
      throw new Error('socket closed')
    })
    await expect(searchDaemonSessions(t, { query: 'x' })).rejects.toThrow('socket closed')
  })
})

describe('fetchDaemonSearchContext', () => {
  it('returns the window and treats unknown sessions as expected staleness', async () => {
    const good = transport(() => ({ lines: ['a', 'b', 'c'], firstAbsRow: 1 }))
    expect(await fetchDaemonSearchContext(good, { sessionId: 's', absRow: 2 })).toEqual({
      lines: ['a', 'b', 'c'],
      firstAbsRow: 1
    })
    const gone = transport(() => {
      throw new Error('unknown session')
    })
    expect(await fetchDaemonSearchContext(gone, { sessionId: 's', absRow: 2 })).toBeNull()
  })
})

describe('cold-session replay handshake', () => {
  const content = { rows: 24, cols: 80, chunks: ['stored bytes'] }
  const hit = { matches: [], total: 1, incomplete: false, needsContent: false }

  it('serves from the generation cache without loading content', async () => {
    const load = vi.fn()
    const t = transport(() => hit)
    const result = await searchDeadSessionHistory(t, {
      sessionId: 'dead',
      generation: 3,
      query: 'q',
      loadContent: load
    })
    expect(result?.total).toBe(1)
    expect(load).not.toHaveBeenCalled()
    expect(t.calls).toHaveLength(1)
  })

  it('ships content exactly once on a cache miss', async () => {
    const load = vi.fn(async () => content)
    const t = transport((_type, payload) =>
      (payload as { content?: unknown }).content ? hit : { needsContent: true }
    )
    const result = await searchDeadSessionHistory(t, {
      sessionId: 'dead',
      generation: 3,
      query: 'q',
      loadContent: load
    })
    expect(result?.total).toBe(1)
    expect(load).toHaveBeenCalledTimes(1)
    expect(t.calls).toHaveLength(2)
    expect(t.calls[1].payload).toMatchObject({ content })
  })

  it('never loops when the daemon keeps asking for content', async () => {
    const t = transport(() => ({ needsContent: true }))
    const result = await searchDeadSessionHistory(t, {
      sessionId: 'dead',
      generation: 3,
      query: 'q',
      loadContent: async () => content
    })
    expect(result).toBeNull()
    expect(t.calls).toHaveLength(2)
  })

  it('returns null when no content can be loaded (nothing persisted)', async () => {
    const t = transport(() => ({ needsContent: true }))
    const result = await fetchDeadSessionSearchContext(t, {
      sessionId: 'dead',
      generation: 1,
      absRow: 0,
      loadContent: async () => null
    })
    expect(result).toBeNull()
    expect(t.calls).toHaveLength(1)
  })
})

describe('buildReplaySearchContent', () => {
  const log: TerminalHistoryLogContents = {
    generation: 2,
    truncatedTail: false,
    batches: [
      {
        seq: 1,
        records: [
          { kind: 'output', data: 'tail output' },
          { kind: 'resize', cols: 100, rows: 30 },
          { kind: 'clear' },
          { kind: 'output', data: 'after clear' }
        ]
      }
    ]
  }

  it('orders checkpoint chunks then matching-generation log records', () => {
    const built = buildReplaySearchContent(checkpoint(), log, { cols: 80, rows: 24 })
    // Normal screen: snapshotAnsi already carries the history — no scrollbackAnsi.
    expect(built.chunks).toEqual(['SNAPSHOT', 'tail output', '\x1b[3J', 'after clear'])
    expect(built.rows).toBe(24)
    expect(built.scrollbackRows).toBe(50_000)
  })

  it('prefixes scrollbackAnsi only on the alternate screen', () => {
    const built = buildReplaySearchContent(
      checkpoint({ modes: { alternateScreen: true } as TerminalCheckpointFile['modes'] }),
      null,
      { cols: 80, rows: 24 }
    )
    expect(built.chunks).toEqual(['SCROLLBACK', 'SNAPSHOT'])
  })

  it('drops a log from a superseded generation', () => {
    const built = buildReplaySearchContent(checkpoint({ generation: 5 }), log, {
      cols: 80,
      rows: 24
    })
    expect(built.chunks).toEqual(['SNAPSHOT'])
  })

  it('accepts a generation-0 log with no checkpoint and uses fallback dims', () => {
    const built = buildReplaySearchContent(null, { ...log, generation: 0 }, {
      cols: 120,
      rows: 40
    })
    expect(built.chunks).toEqual(['tail output', '\x1b[3J', 'after clear'])
    expect(built.cols).toBe(120)
    expect(built.rows).toBe(40)
  })
})
