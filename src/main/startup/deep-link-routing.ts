import {
  MAX_ORCA_DEEP_LINK_LENGTH,
  parseOrcaDeepLink,
  type OrcaDeepLink,
  type OrcaDeepLinkOrigin
} from '../../shared/orca-deep-link'

/** Last `orca://` arg wins; everything else in argv is IGNORED — a second
 *  instance's argv is attacker-influenceable junk (Chromium switches, file
 *  paths) and must never be interpreted. Length-capped BEFORE parse. */
export function extractDeepLinkFromArgv(argv: string[]): string | null {
  for (let index = argv.length - 1; index >= 0; index -= 1) {
    const arg = argv[index]
    if (
      typeof arg === 'string' &&
      arg.length <= MAX_ORCA_DEEP_LINK_LENGTH &&
      /^orca:\/\//i.test(arg)
    ) {
      return arg
    }
  }
  return null
}

export type DeepLinkDispatcher = (link: OrcaDeepLink, origin: OrcaDeepLinkOrigin) => void

export type DeepLinkRouter = {
  routeRaw: (raw: string, origin: OrcaDeepLinkOrigin) => void
  /** Call once the renderer's `ui:*` listeners are attached (startup barrier). */
  drainQueued: () => void
}

export function createDeepLinkRouter(opts: {
  dispatch: DeepLinkDispatcher
  isWindowReady: () => boolean
  requestActivation: () => void
  onUnparseable?: () => void
  /** Queue depth; older entries dropped (rate limit, design §6.3-6.4). */
  maxQueued?: number
  /** OS-routed navigation rate limit: max 1 dispatch per interval. */
  minDispatchIntervalMs?: number
  now?: () => number
  schedule?: (fn: () => void, delayMs: number) => void
}): DeepLinkRouter {
  const maxQueued = opts.maxQueued ?? 4
  const minIntervalMs = opts.minDispatchIntervalMs ?? 300
  const now = opts.now ?? Date.now
  const schedule = opts.schedule ?? ((fn, delayMs) => void setTimeout(fn, delayMs))

  const queue: { link: OrcaDeepLink; origin: OrcaDeepLinkOrigin }[] = []
  let lastDispatchAt = Number.NEGATIVE_INFINITY
  let timerArmed = false

  const drain = (): void => {
    if (!opts.isWindowReady()) {
      return
    }
    while (queue.length > 0) {
      const elapsed = now() - lastDispatchAt
      if (elapsed < minIntervalMs) {
        if (!timerArmed) {
          timerArmed = true
          schedule(() => {
            timerArmed = false
            drain()
          }, minIntervalMs - elapsed)
        }
        return
      }
      const entry = queue.shift()
      if (!entry) {
        return
      }
      lastDispatchAt = now()
      opts.dispatch(entry.link, entry.origin)
    }
  }

  return {
    routeRaw: (raw, origin) => {
      // Length cap BEFORE parse on every entry path (argv, open-url, OSC-8).
      if (typeof raw !== 'string' || raw.length > MAX_ORCA_DEEP_LINK_LENGTH) {
        opts.onUnparseable?.()
        return
      }
      const link = parseOrcaDeepLink(raw)
      if (!link) {
        opts.onUnparseable?.()
        return
      }
      // Why: an OS-routed link must surface the window even while queued —
      // matches the second-instance activation the user just triggered.
      opts.requestActivation()
      queue.push({ link, origin })
      if (queue.length > maxQueued) {
        // Why: a page looping location.href='orca://…' must not thrash focus or
        // DoS the resolver — drop oldest, keep the user's most recent intent.
        queue.shift()
      }
      drain()
    },
    drainQueued: drain
  }
}
