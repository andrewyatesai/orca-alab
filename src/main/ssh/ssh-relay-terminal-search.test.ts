// Fed §2.4 / §6: the SSH mux ROUTE for terminal.search — a REAL
// SshChannelMultiplexer joined to the REAL RelayDispatcher over the wire
// protocol (no fakes on either protocol end), proving: (1) requests traverse
// the mux framing into the relay dispatcher's method table and back, (2) an
// AbortSignal cancels IN FLIGHT — the mux emits rpc.cancel and the relay-side
// request controller aborts (client-request-aborts pattern), (3) a relay
// without the method (-32601) degrades that HOST to "unsupported", (4)
// malformed payloads degrade the pane without poisoning the host cache.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { RelayDispatcher } from '../../relay/dispatcher'
import { SshChannelMultiplexer, type MultiplexerTransport } from './ssh-channel-multiplexer'
import {
  clearSshRelayTerminalSearchSupport,
  resetSshRelayTerminalSearchSupportForTest,
  searchSshRelayTerminal,
  sshRelayTerminalSearchContext
} from './ssh-relay-terminal-search'
import { TERMINAL_REMOTE_SEARCH_SCHEMA_VERSION } from '../../shared/terminal-remote-search-protocol'

/** Loopback: mux transport writes feed the dispatcher; dispatcher writes feed
 *  the mux. Both ends speak the real 13-byte relay framing. */
function connectMuxToDispatcher(): {
  mux: SshChannelMultiplexer
  dispatcher: RelayDispatcher
  dispose: () => void
} {
  const muxDataCallbacks: ((data: Buffer) => void)[] = []
  const dispatcher = new RelayDispatcher((data) => {
    for (const cb of muxDataCallbacks) {
      cb(data)
    }
  })
  const transport: MultiplexerTransport = {
    write: (data) => dispatcher.feed(data),
    onData: (cb) => muxDataCallbacks.push(cb),
    onClose: () => undefined
  }
  const mux = new SshChannelMultiplexer(transport)
  return {
    mux,
    dispatcher,
    dispose: () => {
      mux.dispose('shutdown')
      dispatcher.dispose()
    }
  }
}

const disposers: (() => void)[] = []

afterEach(() => {
  for (const dispose of disposers.splice(0)) {
    dispose()
  }
  resetSshRelayTerminalSearchSupportForTest()
})

function connect(): { mux: SshChannelMultiplexer; dispatcher: RelayDispatcher } {
  const { mux, dispatcher, dispose } = connectMuxToDispatcher()
  disposers.push(dispose)
  return { mux, dispatcher }
}

describe('searchSshRelayTerminal over a real mux ↔ relay dispatcher loopback', () => {
  it('routes the request through the mux into the relay handler and back', async () => {
    const { mux, dispatcher } = connect()
    const seen: unknown[] = []
    dispatcher.onRequest('terminal.search', async (params) => {
      seen.push(params)
      return {
        searchSchema: TERMINAL_REMOTE_SEARCH_SCHEMA_VERSION,
        available: true,
        matches: [{ hostRow: 41, col: 2, len: 6, line: 'x needle y' }],
        total: 1,
        incomplete: false,
        hostCols: 80,
        gen: 7
      }
    })
    const outcome = await searchSshRelayTerminal(mux, 'host-a', {
      terminal: 't1',
      query: 'needle',
      gen: 7
    })
    expect(seen).toEqual([{ terminal: 't1', query: 'needle', gen: 7 }])
    expect(outcome.kind).toBe('results')
    if (outcome.kind === 'results') {
      expect(outcome.result.matches[0]).toMatchObject({ hostRow: 41, col: 2, len: 6 })
      expect(outcome.result.gen).toBe(7)
    }
  })

  it('an AbortSignal cancels IN FLIGHT: rpc.cancel reaches the relay-side controller', async () => {
    const { mux, dispatcher } = connect()
    const handlerSignalAborted = vi.fn()
    let releaseHandler: () => void = () => undefined
    dispatcher.onRequest('terminal.search', async (_params, context) => {
      // Simulate a host-side scan awaiting its writeChain: hold the request
      // open until the abort lands, then report what the signal observed.
      await new Promise<void>((resolve) => {
        releaseHandler = resolve
        context.signal?.addEventListener('abort', () => {
          handlerSignalAborted()
          resolve()
        })
      })
      throw new Error('terminal_search_aborted')
    })
    const controller = new AbortController()
    const pending = searchSshRelayTerminal(
      mux,
      'host-a',
      { terminal: 't1', query: 'needle' },
      { signal: controller.signal }
    )
    // Let the request frame traverse into the dispatcher before aborting.
    await new Promise((r) => setTimeout(r, 0))
    controller.abort()
    const outcome = await pending
    // Client side: released promptly as unreachable (not a thrown error).
    expect(outcome).toEqual({ kind: 'unavailable', reason: 'unreachable' })
    // Relay side: the rpc.cancel notification aborted THIS request's controller.
    await new Promise((r) => setTimeout(r, 0))
    expect(handlerSignalAborted).toHaveBeenCalled()
    releaseHandler()
  })

  it('a relay without the method (-32601) marks the HOST unsupported and degrades', async () => {
    const { mux } = connect() // no terminal.search handler registered
    const first = await searchSshRelayTerminal(mux, 'host-old', {
      terminal: 't1',
      query: 'needle'
    })
    expect(first).toEqual({ kind: 'unavailable', reason: 'unsupported-host' })
    // Cached per host: the second query never re-probes the wire.
    const second = await searchSshRelayTerminal(mux, 'host-old', {
      terminal: 't2',
      query: 'other'
    })
    expect(second).toEqual({ kind: 'unavailable', reason: 'unsupported-host' })
    // A different host is not poisoned; clearing (re-deploy/reconnect) re-probes.
    clearSshRelayTerminalSearchSupport('host-old')
    const third = await searchSshRelayTerminal(mux, 'host-old', {
      terminal: 't1',
      query: 'needle'
    })
    expect(third).toEqual({ kind: 'unavailable', reason: 'unsupported-host' })
  })

  it('malformed payloads degrade the pane WITHOUT caching host unsupport', async () => {
    const { mux, dispatcher } = connect()
    let malformed = true
    dispatcher.onRequest('terminal.search', async () => {
      if (malformed) {
        return { nonsense: true }
      }
      return {
        searchSchema: TERMINAL_REMOTE_SEARCH_SCHEMA_VERSION,
        available: true,
        matches: [],
        total: 0,
        incomplete: false,
        hostCols: 80
      }
    })
    const bad = await searchSshRelayTerminal(mux, 'host-b', { terminal: 't1', query: 'q' })
    expect(bad).toEqual({ kind: 'unavailable', reason: 'pane-unsearchable' })
    malformed = false
    const good = await searchSshRelayTerminal(mux, 'host-b', { terminal: 't1', query: 'q' })
    expect(good.kind).toBe('results')
  })

  it('available:false degrades per-pane (host stays searchable for other panes)', async () => {
    const { mux, dispatcher } = connect()
    dispatcher.onRequest('terminal.search', async (params) => ({
      searchSchema: TERMINAL_REMOTE_SEARCH_SCHEMA_VERSION,
      available: (params as { terminal?: string }).terminal !== 'dead',
      matches: [],
      total: 0,
      incomplete: false,
      hostCols: null
    }))
    const dead = await searchSshRelayTerminal(mux, 'host-c', { terminal: 'dead', query: 'q' })
    expect(dead).toEqual({ kind: 'unavailable', reason: 'pane-unsearchable' })
    const live = await searchSshRelayTerminal(mux, 'host-c', { terminal: 'live', query: 'q' })
    expect(live.kind).toBe('results')
  })

  it('terminal.searchContext rides the same route and degradation', async () => {
    const { mux, dispatcher } = connect()
    dispatcher.onRequest('terminal.searchContext', async (params) => ({
      searchSchema: TERMINAL_REMOTE_SEARCH_SCHEMA_VERSION,
      available: true,
      lines: ['a', 'b', 'c'],
      firstHostRow: (params as { hostRow: number }).hostRow - 1
    }))
    const outcome = await sshRelayTerminalSearchContext(mux, 'host-d', {
      terminal: 't1',
      hostRow: 40,
      before: 1,
      after: 1
    })
    expect(outcome.kind).toBe('context')
    if (outcome.kind === 'context') {
      expect(outcome.result.lines).toEqual(['a', 'b', 'c'])
      expect(outcome.result.firstHostRow).toBe(39)
    }
  })
})
