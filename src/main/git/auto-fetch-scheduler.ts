import type { Repo } from '../../shared/types'
import type { GitAutoFetchSettings } from '../../shared/git-auto-fetch-settings'

// Why: a hung network fetch must release the per-repo in-flight slot; the
// runner kills the subprocess on timeout so retries see a clean state.
export const AUTO_FETCH_TIMEOUT_MS = 2 * 60_000
// Why: failures double the wait per repo (auth-less remotes, offline hosts)
// so a broken remote cannot generate a fetch attempt every interval forever.
const MAX_BACKOFF_MULTIPLIER = 8
// Why: the first fetch waits briefly instead of a full interval so freshly
// enabled auto-fetch (and app startup) produces useful counts soon, without
// stampeding git across every repo during startup.
const INITIAL_DELAY_MS = 30_000
const TICK_MS = 15_000

type RepoFetchState = {
  nextEligibleAt: number
  consecutiveFailures: number
  inFlight: boolean
  // Why: null until the first attempt lands; lets an interval change recompute
  // eligibility from the real last-fetch time instead of the 30s initial delay.
  lastAttemptAt: number | null
}

export type GitAutoFetchSchedulerDeps = {
  /** Repos eligible for fetching; the wiring site owns host routing/filtering. */
  listRepos: () => Repo[]
  fetchRepo: (repo: Repo) => Promise<void>
  onRepoFetched?: (repo: Repo) => void
  now?: () => number
  setIntervalFn?: typeof setInterval
  clearIntervalFn?: typeof clearInterval
}

export class GitAutoFetchScheduler {
  private enabled = false
  private intervalMs = 5 * 60_000
  private timer: ReturnType<typeof setInterval> | null = null
  private readonly repoStates = new Map<string, RepoFetchState>()
  private tickInFlight = false

  constructor(private readonly deps: GitAutoFetchSchedulerDeps) {}

  configure(settings: GitAutoFetchSettings): void {
    const previousIntervalMs = this.intervalMs
    const wasEnabled = this.enabled
    this.enabled = settings.enabled
    this.intervalMs = settings.intervalMinutes * 60_000
    if (!this.enabled) {
      this.stop()
      return
    }
    if (!wasEnabled) {
      // Why: a disable→re-enable is a fresh start, so wipe state and let the next
      // tick re-seed the brief INITIAL_DELAY (no stale mid-interval throttle).
      this.repoStates.clear()
    } else if (this.intervalMs !== previousIntervalMs) {
      // Why: an interval change must re-time already-fetched repos onto the NEW
      // cadence from their real last-fetch time — not reset them to the 30s initial
      // delay, which would fetch a just-fetched repo ~30s after switching to a
      // longer interval, violating that interval.
      this.reseedForIntervalChange()
    }
    if (this.timer) {
      return
    }
    const setIntervalFn = this.deps.setIntervalFn ?? setInterval
    this.timer = setIntervalFn(() => {
      void this.runDueFetches()
    }, TICK_MS)
    // Why: a background fetch cadence must never keep the process alive on quit.
    this.timer.unref?.()
  }

  private reseedForIntervalChange(): void {
    for (const state of this.repoStates.values()) {
      // Why: a repo still in its initial-delay window (no attempt yet) keeps that
      // schedule; the new interval only governs the wait AFTER a fetch.
      if (state.lastAttemptAt === null) {
        continue
      }
      const multiplier = Math.min(2 ** state.consecutiveFailures, MAX_BACKOFF_MULTIPLIER)
      state.nextEligibleAt = state.lastAttemptAt + this.intervalMs * multiplier
    }
  }

  stop(): void {
    if (this.timer) {
      const clearIntervalFn = this.deps.clearIntervalFn ?? clearInterval
      clearIntervalFn(this.timer)
      this.timer = null
    }
  }

  async runDueFetches(): Promise<void> {
    if (!this.enabled || this.tickInFlight) {
      return
    }
    this.tickInFlight = true
    try {
      const now = this.deps.now?.() ?? Date.now()
      for (const repo of this.deps.listRepos()) {
        const state = this.repoStates.get(repo.id) ?? {
          nextEligibleAt: now + INITIAL_DELAY_MS,
          consecutiveFailures: 0,
          inFlight: false,
          lastAttemptAt: null
        }
        this.repoStates.set(repo.id, state)
        if (state.inFlight || now < state.nextEligibleAt) {
          continue
        }
        // Why: sequential fetches avoid a subprocess/network storm on repo fleets.
        await this.fetchRepo(repo, state)
      }
    } finally {
      this.tickInFlight = false
    }
  }

  private async fetchRepo(repo: Repo, state: RepoFetchState): Promise<void> {
    state.inFlight = true
    try {
      await this.deps.fetchRepo(repo)
      state.consecutiveFailures = 0
      const completedAt = this.deps.now?.() ?? Date.now()
      state.lastAttemptAt = completedAt
      state.nextEligibleAt = completedAt + this.intervalMs
      this.deps.onRepoFetched?.(repo)
    } catch {
      state.consecutiveFailures += 1
      const multiplier = Math.min(2 ** state.consecutiveFailures, MAX_BACKOFF_MULTIPLIER)
      const completedAt = this.deps.now?.() ?? Date.now()
      state.lastAttemptAt = completedAt
      state.nextEligibleAt = completedAt + this.intervalMs * multiplier
    } finally {
      state.inFlight = false
    }
  }
}
