import { describe, expect, it, vi } from 'vitest'
import { createAtermSharedWorkerHost } from './aterm-shared-render-worker'
import type { AtermWorkerMessage, AtermWorkerRequest } from './aterm-render-worker-protocol'

// Lifecycle proofs for the SHARED render-worker manager: one worker for N panes,
// fonts sent once per generation, paneId routing, crash retirement (every pane
// recovers, one-shot), terminate-on-last-release, lazy recreation.

type MessageListener = (event: { data: AtermWorkerMessage }) => void
type ErrorListener = (event: { message: string }) => void

function makeFakeWorker() {
  const posted: { message: AtermWorkerRequest; transfer: Transferable[] }[] = []
  let onMessage: MessageListener | null = null
  let onError: ErrorListener | null = null
  const worker = {
    terminated: false,
    postMessage: (message: unknown, transfer: Transferable[]) => {
      posted.push({ message: message as AtermWorkerRequest, transfer })
    },
    terminate: () => {
      worker.terminated = true
    },
    addEventListener: (type: string, listener: unknown) => {
      if (type === 'message') {
        onMessage = listener as MessageListener
      } else {
        onError = listener as ErrorListener
      }
    },
    emit: (data: AtermWorkerMessage) => onMessage?.({ data }),
    emitError: (message: string) => onError?.({ message })
  }
  return { worker, posted }
}

function makeHost() {
  const workers: ReturnType<typeof makeFakeWorker>[] = []
  const fonts = {
    primary: new Uint8Array([1, 2, 3]),
    fallbacks: [new Uint8Array([4, 5])],
    emoji: new Uint8Array([6]),
    symbol: new Uint8Array([7])
  }
  const host = createAtermSharedWorkerHost({
    createWorker: () => {
      const fake = makeFakeWorker()
      workers.push(fake)
      return fake.worker
    },
    loadFonts: () => Promise.resolve(fonts)
  })
  return { host, workers, fonts }
}

describe('aterm shared render worker host', () => {
  it('creates ONE worker for many panes and sends fonts once, before any init', async () => {
    const { host, workers, fonts } = makeHost()
    const a = await host.acquirePane()
    const b = await host.acquirePane()
    expect(workers).toHaveLength(1)
    expect(a.paneId).not.toBe(b.paneId)

    a.post({ type: 'draw' })
    b.post({ type: 'draw' })
    const posted = workers[0].posted
    const fontMessages = posted.filter((p) => p.message.type === 'fonts')
    expect(fontMessages).toHaveLength(1)
    expect(posted[0].message.type).toBe('fonts') // fonts strictly first
    // The sent faces are COPIES (transferable without detaching the renderer cache).
    const sent = fontMessages[0].message as { primary: Uint8Array; symbol?: Uint8Array }
    expect(sent.primary).toEqual(fonts.primary)
    expect(sent.primary).not.toBe(fonts.primary)
    expect(fonts.primary.byteLength, 'renderer cache must stay intact').toBe(3)
    // The monochrome symbol tier rides the same one-shot fonts message (copy + transfer).
    expect(sent.symbol).toEqual(fonts.symbol)
    expect(sent.symbol).not.toBe(fonts.symbol)
    expect(fontMessages[0].transfer).toContain(sent.symbol?.buffer)
    // Pane commands are stamped with each pane's own id.
    const draws = posted.filter((p) => p.message.type === 'draw')
    expect(draws.map((d) => (d.message as { paneId: number }).paneId)).toEqual([a.paneId, b.paneId])
  })

  it('routes pane events by paneId and worker-scoped booted to all', async () => {
    const { host, workers } = makeHost()
    const a = await host.acquirePane()
    const b = await host.acquirePane()
    const seenA = vi.fn()
    const seenB = vi.fn()
    a.onEvent(seenA)
    b.onEvent(seenB)
    expect(a.isBooted()).toBe(false)
    workers[0].worker.emit({ type: 'booted' })
    expect(a.isBooted()).toBe(true)
    expect(b.isBooted()).toBe(true)
    workers[0].worker.emit({ type: 'bell', paneId: b.paneId })
    expect(seenA).not.toHaveBeenCalled()
    expect(seenB).toHaveBeenCalledWith({ type: 'bell', paneId: b.paneId })
  })

  it('a crash retires the worker ONCE: every pane recovers, then posts are no-ops', async () => {
    const { host, workers } = makeHost()
    const a = await host.acquirePane()
    const b = await host.acquirePane()
    const crashA = vi.fn()
    const crashB = vi.fn()
    a.onCrash(crashA)
    b.onCrash(crashB)
    workers[0].worker.emit({ type: 'crash', message: 'wasm panic' })
    expect(crashA).toHaveBeenCalledWith('wasm panic')
    expect(crashB).toHaveBeenCalledWith('wasm panic')
    expect(workers[0].worker.terminated).toBe(true)
    // The follow-up 'error' event for the same failure must not re-fire recovery.
    workers[0].worker.emitError('uncaught worker error')
    expect(crashA).toHaveBeenCalledTimes(1)
    // Posts/releases against the retired generation are safe no-ops.
    const before = workers[0].posted.length
    a.post({ type: 'draw' })
    a.release()
    expect(workers[0].posted.length).toBe(before)
  })

  it('the worker "error" event (script load / escaped exception) also retires it', async () => {
    const { host, workers } = makeHost()
    const a = await host.acquirePane()
    const crashA = vi.fn()
    a.onCrash(crashA)
    workers[0].worker.emitError('failed to load script')
    expect(crashA).toHaveBeenCalledWith('failed to load script')
    expect(workers[0].worker.terminated).toBe(true)
  })

  it('boot-wedge escalation retires the shared worker for every pane on it', async () => {
    const { host, workers } = makeHost()
    const a = await host.acquirePane()
    const b = await host.acquirePane()
    const crashB = vi.fn()
    b.onCrash(crashB)
    a.reportBootWedged('boot timed out')
    expect(crashB).toHaveBeenCalledWith('boot timed out')
    expect(workers[0].worker.terminated).toBe(true)
  })

  it('terminates on last release and lazily recreates (fonts re-sent) for the next pane', async () => {
    const { host, workers } = makeHost()
    const a = await host.acquirePane()
    const b = await host.acquirePane()
    a.release()
    expect(workers[0].worker.terminated, 'a live pane remains').toBe(false)
    b.release()
    expect(workers[0].worker.terminated, 'last release frees the worker').toBe(true)

    const c = await host.acquirePane()
    expect(workers, 'a fresh generation spawns on demand').toHaveLength(2)
    expect(workers[1].posted[0].message.type, 'the new generation gets fonts again').toBe('fonts')
    c.post({ type: 'draw' })
    expect(workers[1].posted.some((p) => p.message.type === 'draw')).toBe(true)
  })
})
