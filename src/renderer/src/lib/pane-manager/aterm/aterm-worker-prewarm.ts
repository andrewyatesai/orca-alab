import { acquireAtermSharedWorkerPane } from './aterm-shared-render-worker'
import { scheduleAfterInputQuiet } from '@/lib/input-quiet-scheduler'
import { e2eConfig } from '@/lib/e2e-config'

// Idle-time prewarm of the SHARED render worker so the FIRST pane of a session
// opens hot instead of paying the documented multi-second cold boot (wasm
// compile + font fetch/IPC + worker spawn — see aterm-worker-loader's
// 4s-boot/15s-first-frame window). Acquiring a slot runs the whole warm path:
// loadAterm (main-thread wasm compile + primary font fetch), the OS fallback
// font IPC, worker spawn/script parse, and the resident-fonts post.
//
// The manager's steady-state policy is memory-over-warmth (terminate on last
// release); prewarm deliberately trades a BOUNDED hold window against it: the
// hold releases the moment a real pane owns the worker, or after HOLD_MS if no
// terminal ever opens. The renderer-side font/wasm caches persist either way,
// so even an expired prewarm leaves the next open warm.
const PREWARM_HOLD_MS = 90_000
// Wait out startup crunch + a quiet input window before spending idle time.
const PREWARM_IDLE_DELAY_MS = 1500
const PREWARM_IDLE_QUIET_MS = 750
const PREWARM_IDLE_TIMEOUT_MS = 8000

export type AtermWorkerPrewarmHold = { release: () => void }

export type AtermWorkerPrewarmDeps = {
  acquire: () => Promise<AtermWorkerPrewarmHold>
  /** Schedule the idle prewarm attempt; returns a canceller. */
  schedule: (run: () => void) => () => void
  holdMs: number
}

export type AtermWorkerPrewarm = {
  arm: () => void
  notePaneAcquired: () => void
}

/** Factory (deps injected) so unit tests can drive the lifecycle with fakes;
 *  production uses the module-level singleton below. */
export function createAtermWorkerPrewarm(deps: AtermWorkerPrewarmDeps): AtermWorkerPrewarm {
  let armed = false
  let paneSeen = false
  let hold: AtermWorkerPrewarmHold | null = null
  let holdTimer: ReturnType<typeof setTimeout> | null = null
  let cancelSchedule: (() => void) | null = null

  const releaseHold = (): void => {
    if (holdTimer !== null) {
      clearTimeout(holdTimer)
      holdTimer = null
    }
    hold?.release()
    hold = null
  }

  return {
    arm: (): void => {
      if (armed) {
        return
      }
      armed = true
      cancelSchedule = deps.schedule(() => {
        cancelSchedule = null
        if (paneSeen) {
          return
        }
        deps
          .acquire()
          .then((pane) => {
            if (paneSeen) {
              // A real pane raced ahead and owns the worker now — the warm-up
              // already happened; drop the redundant slot immediately.
              pane.release()
              return
            }
            hold = pane
            holdTimer = setTimeout(releaseHold, deps.holdMs)
          })
          .catch(() => {
            // Best-effort: a prewarm failure is invisible; the first real pane
            // open runs the same path and surfaces any real error itself.
          })
      })
    },
    notePaneAcquired: (): void => {
      paneSeen = true
      cancelSchedule?.()
      cancelSchedule = null
      // Safe order: the caller's slot is already registered, so releasing the
      // hold can never drop the worker's pane count to zero here.
      releaseHold()
    }
  }
}

const productionPrewarm = createAtermWorkerPrewarm({
  acquire: acquireAtermSharedWorkerPane,
  schedule: (run) =>
    scheduleAfterInputQuiet(run, {
      delayMs: PREWARM_IDLE_DELAY_MS,
      quietMs: PREWARM_IDLE_QUIET_MS,
      idleTimeoutMs: PREWARM_IDLE_TIMEOUT_MS
    }),
  holdMs: PREWARM_HOLD_MS
})

/** Called by the worker loader's boot path when a REAL pane acquires a slot:
 *  releases the prewarm hold (the real pane keeps the worker alive) and stops
 *  any future prewarm — demand now owns the worker lifecycle. */
export function noteRealAtermWorkerPaneAcquired(): void {
  productionPrewarm.notePaneAcquired()
}

// Self-arm with the renderer bundle (this module loads via the static pane-open
// import chain, long before any pane exists). Skipped under unit tests and e2e
// (exposeStore): specs assert on lazy worker creation/termination and must not
// see a background worker they didn't open.
if (
  typeof window !== 'undefined' &&
  typeof Worker !== 'undefined' &&
  import.meta.env?.MODE !== 'test' &&
  !e2eConfig.exposeStore
) {
  productionPrewarm.arm()
}
