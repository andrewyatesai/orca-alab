import { describe, expect, it, vi } from 'vitest'
import type { OrcaDeepLink, OrcaDeepLinkOrigin } from '../../shared/orca-deep-link'
import { MAX_ORCA_DEEP_LINK_LENGTH } from '../../shared/orca-deep-link'
import { createDeepLinkRouter, extractDeepLinkFromArgv } from './deep-link-routing'

const OS_ORIGIN: OrcaDeepLinkOrigin = { source: 'os' }

function makeRouter(overrides?: {
  ready?: boolean
  maxQueued?: number
  minDispatchIntervalMs?: number
}): {
  router: ReturnType<typeof createDeepLinkRouter>
  dispatched: { link: OrcaDeepLink; origin: OrcaDeepLinkOrigin }[]
  requestActivation: ReturnType<typeof vi.fn>
  onUnparseable: ReturnType<typeof vi.fn>
  setReady: (ready: boolean) => void
  advance: (ms: number) => void
} {
  const dispatched: { link: OrcaDeepLink; origin: OrcaDeepLinkOrigin }[] = []
  const requestActivation = vi.fn()
  const onUnparseable = vi.fn()
  let ready = overrides?.ready ?? true
  let clock = 0
  const timers: { at: number; fn: () => void }[] = []
  const router = createDeepLinkRouter({
    dispatch: (link, origin) => dispatched.push({ link, origin }),
    isWindowReady: () => ready,
    requestActivation,
    onUnparseable,
    maxQueued: overrides?.maxQueued,
    minDispatchIntervalMs: overrides?.minDispatchIntervalMs ?? 0,
    now: () => clock,
    schedule: (fn, delayMs) => timers.push({ at: clock + delayMs, fn })
  })
  return {
    router,
    dispatched,
    requestActivation,
    onUnparseable,
    setReady: (value) => {
      ready = value
    },
    advance: (ms) => {
      clock += ms
      const due = timers.filter((t) => t.at <= clock)
      timers.length = 0
      for (const t of due) {
        t.fn()
      }
    }
  }
}

describe('extractDeepLinkFromArgv', () => {
  it('extracts orca url from windows argv noise', () => {
    expect(
      extractDeepLinkFromArgv([
        'C:\\Program Files\\Orca\\Orca.exe',
        '--allowed-ips=0.0.0.0',
        'C:\\some\\file.txt',
        'orca://focus/term_abc'
      ])
    ).toBe('orca://focus/term_abc')
  })

  it('takes the LAST orca url when several are present', () => {
    expect(
      extractDeepLinkFromArgv(['exe', 'orca://focus/term_first', 'ORCA://focus/term_second'])
    ).toBe('ORCA://focus/term_second')
  })

  it('ignores non-orca argv entries', () => {
    expect(extractDeepLinkFromArgv(['exe', '--serve', 'https://example.com', 'orca:pair'])).toBe(
      null
    )
  })

  it('ignores oversized orca args (length cap before parse)', () => {
    const oversized = `orca://focus/${'a'.repeat(MAX_ORCA_DEEP_LINK_LENGTH)}`
    expect(extractDeepLinkFromArgv(['exe', oversized])).toBe(null)
  })
})

describe('createDeepLinkRouter', () => {
  it('dispatches a parsed link immediately when the window is ready', () => {
    const { router, dispatched, requestActivation } = makeRouter()

    router.routeRaw('orca://focus/term_abc', OS_ORIGIN)

    expect(dispatched).toEqual([{ link: { kind: 'focus', handle: 'term_abc' }, origin: OS_ORIGIN }])
    expect(requestActivation).toHaveBeenCalledTimes(1)
  })

  it('queues links until window ready then drains in order', () => {
    const { router, dispatched, setReady } = makeRouter({ ready: false })

    router.routeRaw('orca://focus/term_one', OS_ORIGIN)
    router.routeRaw('orca://focus/term_two', OS_ORIGIN)
    expect(dispatched).toHaveLength(0)

    setReady(true)
    router.drainQueued()

    expect(dispatched.map((d) => (d.link.kind === 'focus' ? d.link.handle : ''))).toEqual([
      'term_one',
      'term_two'
    ])
  })

  it('open-url before ready is not lost', () => {
    const { router, dispatched, setReady } = makeRouter({ ready: false })

    router.routeRaw('orca://focus/term_early', OS_ORIGIN)
    setReady(true)
    router.drainQueued()

    expect(dispatched).toHaveLength(1)
  })

  it('drops queue overflow beyond maxQueued (oldest first)', () => {
    const { router, dispatched, setReady } = makeRouter({ ready: false, maxQueued: 2 })

    router.routeRaw('orca://focus/term_a', OS_ORIGIN)
    router.routeRaw('orca://focus/term_b', OS_ORIGIN)
    router.routeRaw('orca://focus/term_c', OS_ORIGIN)

    setReady(true)
    router.drainQueued()

    expect(dispatched.map((d) => (d.link.kind === 'focus' ? d.link.handle : ''))).toEqual([
      'term_b',
      'term_c'
    ])
  })

  it('rate-limits dispatches to one per interval', () => {
    const { router, dispatched, advance } = makeRouter({ minDispatchIntervalMs: 300 })

    router.routeRaw('orca://focus/term_a', OS_ORIGIN)
    router.routeRaw('orca://focus/term_b', OS_ORIGIN)
    expect(dispatched).toHaveLength(1)

    advance(300)
    expect(dispatched).toHaveLength(2)
  })

  it('reports unparseable and oversized links without dispatching', () => {
    const { router, dispatched, onUnparseable, requestActivation } = makeRouter()

    router.routeRaw('orca://unknown-host/x', OS_ORIGIN)
    router.routeRaw('x'.repeat(MAX_ORCA_DEEP_LINK_LENGTH + 1), OS_ORIGIN)

    expect(dispatched).toHaveLength(0)
    expect(onUnparseable).toHaveBeenCalledTimes(2)
    // Why: a rejected link must not steal focus either.
    expect(requestActivation).not.toHaveBeenCalled()
  })
})
