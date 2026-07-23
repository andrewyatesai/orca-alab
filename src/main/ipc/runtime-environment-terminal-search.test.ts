// Old-host per-pane degradation (fed §2.4): a host without terminal.search
// answers method_not_found → every pane on THAT host reads "source
// unavailable" without re-probing, other hosts keep searching, transport
// failures never poison the capability cache, and aborted requests never
// surface stale results.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { RuntimeRpcResponse } from '../../shared/runtime-rpc-envelope'
import {
  clearRemoteTerminalSearchSupport,
  resetRemoteTerminalSearchSupportForTest,
  runtimeEnvironmentTerminalSearchContext,
  searchRuntimeEnvironmentTerminal,
  setRuntimeEnvironmentTerminalSearchTransportForTest
} from './runtime-environment-terminal-search'

const META = { runtimeId: 'r-1' }

function ok(result: unknown): RuntimeRpcResponse<unknown> {
  return { id: 'x', ok: true, result, _meta: META } as RuntimeRpcResponse<unknown>
}

function err(code: string): RuntimeRpcResponse<unknown> {
  return {
    id: 'x',
    ok: false,
    error: { code, message: code },
    _meta: META
  } as RuntimeRpcResponse<unknown>
}

const RESULT = {
  searchSchema: 1,
  available: true,
  matches: [{ hostRow: 10, col: 0, len: 3, line: 'abc' }],
  total: 1,
  incomplete: false,
  hostCols: 80
}

describe('searchRuntimeEnvironmentTerminal', () => {
  const transport = vi.fn()

  beforeEach(() => {
    transport.mockReset()
    resetRemoteTerminalSearchSupportForTest()
    setRuntimeEnvironmentTerminalSearchTransportForTest(transport)
  })

  afterEach(() => {
    setRuntimeEnvironmentTerminalSearchTransportForTest(null)
  })

  it('returns results from a supporting host', async () => {
    transport.mockResolvedValue(ok(RESULT))
    const outcome = await searchRuntimeEnvironmentTerminal('/data', 'env-1', {
      terminal: 't-1',
      query: 'abc'
    })
    expect(outcome).toEqual({ kind: 'results', result: RESULT })
    expect(transport).toHaveBeenCalledWith(
      '/data',
      'env-1',
      'terminal.search',
      { terminal: 't-1', query: 'abc' },
      expect.any(Number)
    )
  })

  it('classifies method_not_found as unsupported-host and caches it per environment', async () => {
    transport.mockResolvedValue(err('method_not_found'))
    const first = await searchRuntimeEnvironmentTerminal('/data', 'env-old', {
      terminal: 't-1',
      query: 'abc'
    })
    expect(first).toEqual({ kind: 'unavailable', reason: 'unsupported-host' })
    // Second pane, same host: no re-probe (per-host cache, per-pane verdict).
    const second = await searchRuntimeEnvironmentTerminal('/data', 'env-old', {
      terminal: 't-2',
      query: 'abc'
    })
    expect(second).toEqual({ kind: 'unavailable', reason: 'unsupported-host' })
    expect(transport).toHaveBeenCalledTimes(1)
    // A DIFFERENT host is not poisoned by env-old's verdict.
    transport.mockResolvedValue(ok(RESULT))
    const other = await searchRuntimeEnvironmentTerminal('/data', 'env-new', {
      terminal: 't-1',
      query: 'abc'
    })
    expect(other.kind).toBe('results')
  })

  it('clears the unsupported verdict on demand (host upgrade/re-pair)', async () => {
    transport.mockResolvedValue(err('method_not_found'))
    await searchRuntimeEnvironmentTerminal('/data', 'env-old', { terminal: 't', query: 'q' })
    clearRemoteTerminalSearchSupport('env-old')
    transport.mockResolvedValue(ok(RESULT))
    const outcome = await searchRuntimeEnvironmentTerminal('/data', 'env-old', {
      terminal: 't',
      query: 'q'
    })
    expect(outcome.kind).toBe('results')
    expect(transport).toHaveBeenCalledTimes(2)
  })

  it('treats transport failures as unreachable WITHOUT caching unsupported', async () => {
    transport.mockRejectedValueOnce(new Error('connect refused'))
    const first = await searchRuntimeEnvironmentTerminal('/data', 'env-1', {
      terminal: 't',
      query: 'q'
    })
    expect(first).toEqual({ kind: 'unavailable', reason: 'unreachable' })
    transport.mockResolvedValue(ok(RESULT))
    const second = await searchRuntimeEnvironmentTerminal('/data', 'env-1', {
      terminal: 't',
      query: 'q'
    })
    expect(second.kind).toBe('results')
  })

  it('treats handler errors (not method_not_found) as unreachable without caching', async () => {
    transport.mockResolvedValueOnce(err('terminal_not_found'))
    const outcome = await searchRuntimeEnvironmentTerminal('/data', 'env-1', {
      terminal: 't',
      query: 'q'
    })
    expect(outcome).toEqual({ kind: 'unavailable', reason: 'unreachable' })
    transport.mockResolvedValue(ok(RESULT))
    const again = await searchRuntimeEnvironmentTerminal('/data', 'env-1', {
      terminal: 't',
      query: 'q'
    })
    expect(again.kind).toBe('results')
  })

  it('maps available:false to a per-pane unavailable verdict without caching the host', async () => {
    transport.mockResolvedValueOnce(ok({ ...RESULT, available: false, matches: [] }))
    const outcome = await searchRuntimeEnvironmentTerminal('/data', 'env-1', {
      terminal: 't-unsearchable',
      query: 'q'
    })
    expect(outcome).toEqual({ kind: 'unavailable', reason: 'pane-unsearchable' })
    transport.mockResolvedValue(ok(RESULT))
    const sibling = await searchRuntimeEnvironmentTerminal('/data', 'env-1', {
      terminal: 't-live',
      query: 'q'
    })
    expect(sibling.kind).toBe('results')
  })

  it('rejects unparseable result shapes without caching the host unsupported', async () => {
    transport.mockResolvedValueOnce(ok({ nonsense: true }))
    const outcome = await searchRuntimeEnvironmentTerminal('/data', 'env-1', {
      terminal: 't',
      query: 'q'
    })
    expect(outcome).toEqual({ kind: 'unavailable', reason: 'pane-unsearchable' })
    transport.mockResolvedValue(ok(RESULT))
    const again = await searchRuntimeEnvironmentTerminal('/data', 'env-1', {
      terminal: 't',
      query: 'q'
    })
    expect(again.kind).toBe('results')
  })

  it('never surfaces results for a request aborted mid-flight (Esc / generation bump)', async () => {
    const controller = new AbortController()
    transport.mockImplementation(async () => {
      controller.abort()
      return ok(RESULT)
    })
    const outcome = await searchRuntimeEnvironmentTerminal(
      '/data',
      'env-1',
      { terminal: 't', query: 'q' },
      { signal: controller.signal }
    )
    expect(outcome).toEqual({ kind: 'unavailable', reason: 'unreachable' })
  })

  it('short-circuits an already-aborted request without touching the transport', async () => {
    const controller = new AbortController()
    controller.abort()
    const outcome = await searchRuntimeEnvironmentTerminal(
      '/data',
      'env-1',
      { terminal: 't', query: 'q' },
      { signal: controller.signal }
    )
    expect(outcome).toEqual({ kind: 'unavailable', reason: 'unreachable' })
    expect(transport).not.toHaveBeenCalled()
  })
})

describe('runtimeEnvironmentTerminalSearchContext', () => {
  const transport = vi.fn()

  beforeEach(() => {
    transport.mockReset()
    resetRemoteTerminalSearchSupportForTest()
    setRuntimeEnvironmentTerminalSearchTransportForTest(transport)
  })

  afterEach(() => {
    setRuntimeEnvironmentTerminalSearchTransportForTest(null)
  })

  it('returns the context window and shares the unsupported-host cache with search', async () => {
    transport.mockResolvedValue(
      ok({ searchSchema: 1, available: true, lines: ['a', 'b'], firstHostRow: 9 })
    )
    const outcome = await runtimeEnvironmentTerminalSearchContext('/data', 'env-1', {
      terminal: 't',
      hostRow: 10,
      before: 1,
      after: 0
    })
    expect(outcome).toEqual({
      kind: 'context',
      result: { searchSchema: 1, available: true, lines: ['a', 'b'], firstHostRow: 9 }
    })
    // Mark host unsupported through the search path; context must short-circuit.
    transport.mockResolvedValue(err('method_not_found'))
    await searchRuntimeEnvironmentTerminal('/data', 'env-1', { terminal: 't', query: 'q' })
    transport.mockClear()
    const degraded = await runtimeEnvironmentTerminalSearchContext('/data', 'env-1', {
      terminal: 't',
      hostRow: 10
    })
    expect(degraded).toEqual({ kind: 'unavailable', reason: 'unsupported-host' })
    expect(transport).not.toHaveBeenCalled()
  })
})
