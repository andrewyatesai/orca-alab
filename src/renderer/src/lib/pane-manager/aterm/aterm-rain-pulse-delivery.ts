import {
  bufferAtermRainPulse,
  EMPTY_ATERM_RAIN_PULSE_BUFFER,
  resumeAtermRainPulses,
  type AtermRainPulse,
  type AtermRainPulseBuffer
} from '../../../../../shared/aterm-rain-signal'
import { parsePaneKey } from '../../../../../shared/stable-pane-id'
import {
  getLivePaneManagersForTab,
  getRegisteredTabIdsForController,
  setTabPaneManagerLifecycleObserver
} from '../pane-manager-registry'

// One strongest phase/outcome plus one turn boundary per pane. The global cap
// prevents malformed/stale hook identities from growing renderer memory.
const MAX_PENDING_RAIN_PANES = 64
const pendingByPaneKey = new Map<string, AtermRainPulseBuffer>()

type ParsedPaneKey = NonNullable<ReturnType<typeof parsePaneKey>>

function deliverToRegisteredControllers(parsed: ParsedPaneKey, pulse: AtermRainPulse): boolean {
  const deliveredControllers = new Set<object>()
  let delivered = false
  for (const manager of getLivePaneManagersForTab(parsed.tabId)) {
    let controller: { noteMatrixRainPulse?: (nextPulse: AtermRainPulse) => void } | null | undefined
    try {
      controller = manager
        .getPanes()
        .find((candidate) => candidate.leafId === parsed.leafId)?.atermController
    } catch {
      // A replacement lifecycle can overlap a manager already tearing down.
      // Status IPC must continue to the remaining live manager(s).
      continue
    }
    if (
      !controller ||
      deliveredControllers.has(controller) ||
      typeof controller.noteMatrixRainPulse !== 'function'
    ) {
      continue
    }
    deliveredControllers.add(controller)
    try {
      controller.noteMatrixRainPulse(pulse)
      delivered = true
    } catch {
      // Semantic effects are best-effort across artifact version skew. Never
      // abort the accepted agent-status/title/notification IPC path.
    }
  }
  return delivered
}

function retainPendingPulse(paneKey: string, pulse: AtermRainPulse): void {
  const current = pendingByPaneKey.get(paneKey) ?? EMPTY_ATERM_RAIN_PULSE_BUFFER
  // Refresh insertion order so the least-recently-active missing pane is evicted.
  pendingByPaneKey.delete(paneKey)
  if (pendingByPaneKey.size >= MAX_PENDING_RAIN_PANES) {
    const oldest = pendingByPaneKey.keys().next().value as string | undefined
    if (oldest !== undefined) {
      pendingByPaneKey.delete(oldest)
    }
  }
  pendingByPaneKey.set(paneKey, bufferAtermRainPulse(current, pulse))
}

function flushPendingFor(tabId: string, leafId?: string): void {
  // Snapshot before delivery: a failed optional/skewed controller may re-retain
  // an entry, and must not perturb this bounded scan.
  const pendingSnapshot = Array.from(pendingByPaneKey.entries())
  for (const [paneKey, buffered] of pendingSnapshot) {
    const parsed = parsePaneKey(paneKey)
    if (!parsed || parsed.tabId !== tabId || (leafId !== undefined && parsed.leafId !== leafId)) {
      continue
    }
    pendingByPaneKey.delete(paneKey)
    const pulses = resumeAtermRainPulses(buffered)
    for (let index = 0; index < pulses.length; index++) {
      const pulse = pulses[index]
      if (pulse && deliverToRegisteredControllers(parsed, pulse)) {
        continue
      }
      // Preserve only the undelivered suffix. This remains at most the same
      // two payload-free facts and avoids replaying a turn already delivered.
      for (const remaining of pulses.slice(index)) {
        retainPendingPulse(paneKey, remaining)
      }
      break
    }
  }
}

setTabPaneManagerLifecycleObserver({
  managerRegistered: (tabId) => flushPendingFor(tabId)
})

/** Deliver an accepted live hook pulse to its exact terminal pane. No content
 *  crosses this seam; the engine already owns the visible literal glyph tape. */
export function deliverAtermRainPulse(paneKey: string, pulse: AtermRainPulse): boolean {
  const parsed = parsePaneKey(paneKey)
  if (!parsed) {
    return false
  }
  if (deliverToRegisteredControllers(parsed, pulse)) {
    return true
  }
  retainPendingPulse(paneKey, pulse)
  return false
}

/** Flush a mount-gap pulse at the exact async controller attach edge. The UUID
 * identifies the pane; registry identity matching recovers its durable tab id. */
export function flushPendingAtermRainPulsesAtControllerAttach(
  leafId: string,
  controller: object
): void {
  for (const tabId of getRegisteredTabIdsForController(leafId, controller)) {
    flushPendingFor(tabId, leafId)
  }
}
