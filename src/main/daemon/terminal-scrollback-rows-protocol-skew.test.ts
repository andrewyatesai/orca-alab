import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { mkdtempSync, rmSync } from 'node:fs'
import { DaemonClient } from './client'
import { DaemonServer } from './daemon-server'
import { TerminalHost } from './terminal-host'
import { getDaemonSocketPath } from './daemon-spawner'
import type { SubprocessHandle } from './session'

// P4 protocol skew: `scrollbackRows` is optional on the createOrAttach wire — a
// pre-field client omits it, a hostile/skewed client can send anything, and
// either way the daemon must produce a working session with predictable
// retention. Covers the NDJSON control plane end-to-end (client → server →
// TerminalHost → Session → aterm emulator) plus the host-level forwarding.

function createMockSubprocess(): SubprocessHandle & { _simulateData: (data: string) => void } {
  let onDataCb: ((data: string) => void) | null = null
  let onExitCb: ((code: number) => void) | null = null
  return {
    pid: 55555,
    getForegroundProcess: vi.fn(() => null),
    write: vi.fn(),
    resize: vi.fn(),
    // Why the exit callbacks: host.dispose() awaits physical exit; a mock that never exits stalls teardown for 8s per session.
    kill: vi.fn(() => setTimeout(() => onExitCb?.(0), 5)),
    forceKill: vi.fn(() => onExitCb?.(137)),
    signal: vi.fn(),
    onData(cb) {
      onDataCb = cb
    },
    onExit(cb) {
      onExitCb = cb
    },
    dispose: vi.fn(),
    _simulateData(data: string) {
      onDataCb?.(data)
    }
  }
}

describe('TerminalHost scrollbackRows retention (P4)', () => {
  let host: TerminalHost
  let lastSub: ReturnType<typeof createMockSubprocess>

  beforeEach(() => {
    host = new TerminalHost({
      spawnSubprocess: () => {
        lastSub = createMockSubprocess()
        return lastSub
      }
    })
  })

  afterEach(async () => {
    await host.dispose()
  })

  // Why poll: the session emulator parses subprocess data asynchronously.
  async function waitForScrollbackLines(
    sessionId: string,
    reached: (lines: number) => boolean
  ): Promise<number> {
    const startedAt = Date.now()
    for (;;) {
      const lines = host.getSnapshot(sessionId)?.scrollbackLines ?? 0
      if (reached(lines) || Date.now() - startedAt > 5_000) {
        return lines
      }
      await new Promise((r) => setTimeout(r, 20))
    }
  }

  it('forwards scrollbackRows to session retention (kills the silent 5k cap)', async () => {
    await host.createOrAttach({
      sessionId: 's1',
      cols: 80,
      rows: 24,
      scrollbackRows: 6_000,
      streamClient: { onData: vi.fn(), onExit: vi.fn() }
    })

    // 6100 scrolled-off lines exceed the historical 5000-row default.
    lastSub._simulateData('x\r\n'.repeat(6_124))
    const lines = await waitForScrollbackLines('s1', (l) => l > 5_000)
    expect(lines).toBe(6_000)
  })

  it('keeps the historical 5k retention default when scrollbackRows is absent', async () => {
    await host.createOrAttach({
      sessionId: 's1',
      cols: 80,
      rows: 24,
      streamClient: { onData: vi.fn(), onExit: vi.fn() }
    })

    lastSub._simulateData('x\r\n'.repeat(5_500))
    const lines = await waitForScrollbackLines('s1', (l) => l >= 5_000)
    expect(lines).toBe(5_000)
  })
})

describe('createOrAttach scrollbackRows protocol skew (P4)', () => {
  let dir: string
  let socketPath: string
  let tokenPath: string
  let server: DaemonServer
  let client: DaemonClient
  let lastSub: ReturnType<typeof createMockSubprocess>

  beforeEach(async () => {
    dir = mkdtempSync(join(tmpdir(), 'daemon-scrollback-skew-'))
    socketPath = getDaemonSocketPath(dir)
    tokenPath = join(dir, 'test.token')
    server = new DaemonServer({
      socketPath,
      tokenPath,
      spawnSubprocess: () => {
        lastSub = createMockSubprocess()
        return lastSub
      }
    })
    await server.start()
    client = new DaemonClient({ socketPath, tokenPath })
    await client.ensureConnected()
  })

  afterEach(async () => {
    client?.disconnect()
    await server?.shutdown()
    rmSync(dir, { recursive: true, force: true })
  })

  async function retainedScrollbackLines(
    sessionId: string,
    reached: (lines: number) => boolean
  ): Promise<number> {
    // 1300 scrolled-off lines exceed the 1000-row policy floor but stay under
    // the 5000-row default, so the two retentions are distinguishable.
    lastSub._simulateData('x\r\n'.repeat(1_300))
    const startedAt = Date.now()
    for (;;) {
      const result = await client.request<{ snapshot: { scrollbackLines: number } | null }>(
        'getSnapshot',
        { sessionId }
      )
      const lines = result.snapshot?.scrollbackLines ?? 0
      if (reached(lines) || Date.now() - startedAt > 4_000) {
        return lines
      }
      await new Promise((r) => setTimeout(r, 20))
    }
  }

  it('keeps the historical default retention when the field is absent (pre-field client)', async () => {
    await client.request('createOrAttach', { sessionId: 'skew-absent', cols: 80, rows: 24 })

    const lines = await retainedScrollbackLines('skew-absent', (l) => l > 1_000)
    expect(lines).toBeGreaterThan(1_000)
    expect(lines).toBeLessThanOrEqual(1_300)
  })

  it('applies a valid scrollbackRows to session retention', async () => {
    await client.request('createOrAttach', {
      sessionId: 'skew-valid',
      cols: 80,
      rows: 24,
      scrollbackRows: 1_000
    })

    expect(await retainedScrollbackLines('skew-valid', (l) => l === 1_000)).toBe(1_000)
  })

  it('clamps out-of-policy scrollbackRows instead of failing the RPC', async () => {
    await expect(
      client.request('createOrAttach', {
        sessionId: 'skew-clamp',
        cols: 80,
        rows: 24,
        scrollbackRows: 5
      })
    ).resolves.toMatchObject({ isNew: true })

    expect(await retainedScrollbackLines('skew-clamp', (l) => l === 1_000)).toBe(1_000)
  })

  it('treats a non-numeric scrollbackRows as absent (hostile/skewed client)', async () => {
    await expect(
      client.request('createOrAttach', {
        sessionId: 'skew-junk',
        cols: 80,
        rows: 24,
        scrollbackRows: 'junk' as unknown as number
      })
    ).resolves.toMatchObject({ isNew: true })

    const lines = await retainedScrollbackLines('skew-junk', (l) => l > 1_000)
    expect(lines).toBeGreaterThan(1_000)
  })
})
