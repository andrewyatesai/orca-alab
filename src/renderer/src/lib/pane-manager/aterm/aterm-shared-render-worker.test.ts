import { describe, expect, it, vi } from 'vitest'
import { createAtermSharedWorkerHost } from './aterm-shared-render-worker'
import type { AtermWorkerMessage, AtermWorkerRequest } from './aterm-render-worker-protocol'

// Lifecycle proofs for the SHARED render-worker manager: one worker for N panes,
// the primary-only boot fonts sent once per generation, LAZY font-class delivery
// on the worker's miss signal (E1: once per class per generation, buffers
// transferred, no renderer byte cache), paneId routing, crash retirement (every
// pane recovers, one-shot), terminate-on-last-release, lazy recreation.

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

function makeHost(
  loadFontClass?: (cls: 'text' | 'emoji') => Promise<{
    fallbacks?: Uint8Array[]
    symbol?: Uint8Array
    emoji?: Uint8Array
  }>
) {
  const workers: ReturnType<typeof makeFakeWorker>[] = []
  const fonts = { primary: new Uint8Array([1, 2, 3]) }
  const classFaces = {
    fallbacks: [new Uint8Array([4, 5])],
    symbol: new Uint8Array([7]),
    emoji: new Uint8Array([6])
  }
  const loadClass = vi.fn(
    loadFontClass ??
      ((cls: 'text' | 'emoji') =>
        Promise.resolve(
          cls === 'text'
            ? { fallbacks: classFaces.fallbacks, symbol: classFaces.symbol }
            : { emoji: classFaces.emoji }
        ))
  )
  const host = createAtermSharedWorkerHost({
    createWorker: () => {
      const fake = makeFakeWorker()
      workers.push(fake)
      return fake.worker
    },
    loadFonts: () => Promise.resolve(fonts),
    loadFontClass: loadClass
  })
  return { host, workers, fonts, classFaces, loadClass }
}

/** Let the async deliverFontClass chain settle. */
const settle = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0))

describe('aterm shared render worker host', () => {
  it('creates ONE worker for many panes and sends the primary-only fonts once, first', async () => {
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
    // The boot payload is the PRIMARY only (E1: fallback classes are lazy), sent
    // as a COPY so the transfer can't detach the cached asset bytes.
    const sent = fontMessages[0].message as { primary: Uint8Array; fallbacks: Uint8Array[] }
    expect(sent.primary).toEqual(fonts.primary)
    expect(sent.primary).not.toBe(fonts.primary)
    expect(fonts.primary.byteLength, 'cached asset bytes must stay intact').toBe(3)
    expect(sent.fallbacks).toEqual([])
    // Pane commands are stamped with each pane's own id.
    const draws = posted.filter((p) => p.message.type === 'draw')
    expect(draws.map((d) => (d.message as { paneId: number }).paneId)).toEqual([a.paneId, b.paneId])
  })

  it('delivers a missed font class once per generation, buffers transferred', async () => {
    const { host, workers, classFaces, loadClass } = makeHost()
    await host.acquirePane()
    const worker = workers[0]
    worker.worker.emit({ type: 'missingFontClasses', classes: ['text'] })
    await settle()
    expect(loadClass).toHaveBeenCalledWith('text')
    const deliveries = worker.posted.filter((p) => p.message.type === 'fontClass')
    expect(deliveries).toHaveLength(1)
    const sent = deliveries[0].message as {
      class: string
      fallbacks?: Uint8Array[]
      symbol?: Uint8Array
    }
    expect(sent.class).toBe('text')
    expect(sent.fallbacks?.[0]).toEqual(classFaces.fallbacks[0])
    expect(sent.symbol).toEqual(classFaces.symbol)
    expect(deliveries[0].transfer).toContain(sent.fallbacks?.[0].buffer)
    expect(deliveries[0].transfer).toContain(sent.symbol?.buffer)

    // A re-fired miss for the same class is latched (no duplicate fetch/delivery).
    worker.worker.emit({ type: 'missingFontClasses', classes: ['text'] })
    await settle()
    expect(loadClass).toHaveBeenCalledTimes(1)

    // The emoji class is independent and carries the colour face.
    worker.worker.emit({ type: 'missingFontClasses', classes: ['emoji'] })
    await settle()
    const emojiDelivery = worker.posted.filter((p) => p.message.type === 'fontClass')[1]
    expect((emojiDelivery.message as { emoji?: Uint8Array }).emoji).toEqual(classFaces.emoji)
  })

  it('a failed class fetch is latched for the generation (no delivery, no crash)', async () => {
    const { host, workers, loadClass } = makeHost(() => Promise.reject(new Error('no ipc')))
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    try {
      await host.acquirePane()
      const worker = workers[0]
      worker.worker.emit({ type: 'missingFontClasses', classes: ['text'] })
      worker.worker.emit({ type: 'missingFontClasses', classes: ['text'] })
      await settle()
      expect(loadClass).toHaveBeenCalledTimes(1)
      expect(worker.posted.filter((p) => p.message.type === 'fontClass')).toHaveLength(0)
      expect(warn).toHaveBeenCalled()
    } finally {
      warn.mockRestore()
    }
  })

  it('a class delivery for a retired generation is dropped', async () => {
    const { host, workers } = makeHost()
    const a = await host.acquirePane()
    a.onCrash(() => {})
    const worker = workers[0]
    worker.worker.emit({ type: 'missingFontClasses', classes: ['text'] })
    worker.worker.emit({ type: 'crash', message: 'wasm panic' })
    await settle()
    expect(worker.posted.filter((p) => p.message.type === 'fontClass')).toHaveLength(0)
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

  it("a text fontClass delivery carries the user's fallback stack before the CJK face (PC-8367)", async () => {
    // The production class loader translates the IPC payload into the worker's
    // ordered `fallbacks` array; the worker registry applies it in array order.
    const { loadSharedWorkerFontClass } = await import('./aterm-shared-render-worker')
    const userA = new Uint8Array([0x0a])
    const userB = new Uint8Array([0x0b])
    const cjk = new Uint8Array([0x1c])
    const chain0 = new Uint8Array([0x2c])
    vi.stubGlobal('window', {
      api: {
        fonts: {
          getTerminalFallbackFonts: vi.fn().mockResolvedValue({
            user: [
              { family: 'A', bytes: userA },
              { family: 'B', bytes: userB }
            ],
            cjk: { bytes: cjk, region: 'zh-Hans' },
            chain: [{ bytes: chain0, script: 'arabic' }],
            symbol: new Uint8Array([0x3c])
          })
        }
      }
    })
    try {
      const faces = await loadSharedWorkerFontClass('text')
      expect(faces.fallbacks?.map((face) => face[0])).toEqual([0x0a, 0x0b, 0x1c, 0x2c])
      expect(faces.symbol?.[0]).toBe(0x3c)
    } finally {
      vi.unstubAllGlobals()
    }
  })
})
