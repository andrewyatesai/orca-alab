// Main-side buffers for the render worker's PUSHED edge-triggered side channels
// (OSC app-events, OSC 9/99/777 desktop notifications, BEL). The loader pushes
// worker events in as they arrive; the facade pull-drains them through the
// worker-backed term's take_osc_events/take_notifications/drain_bell, mirroring
// the in-process engine's own drains. Extracted from aterm-worker-term to keep
// that file under the line budget.

export type WorkerSideChannelBuffers = {
  pushOsc: (eventsJson: string) => void
  pushNotifications: (eventsJson: string) => void
  pushBell: () => void
  takeOscEvents: () => string | undefined
  takeNotifications: () => string | undefined
  drainBell: () => boolean
}

/** `notify` fires after every push so the facade drains the fresh channel
 *  immediately instead of waiting for the next process() chunk. */
export function createWorkerSideChannelBuffers(notify: () => void): WorkerSideChannelBuffers {
  let oscEvents: [number, string][] = []
  let pendingNotifications: unknown[] = []
  let bellPending = false
  return {
    pushOsc: (eventsJson) => {
      try {
        oscEvents.push(...(JSON.parse(eventsJson) as [number, string][]))
      } catch {
        /* malformed OSC payload — drop */
      }
      notify()
    },
    pushNotifications: (eventsJson) => {
      try {
        pendingNotifications.push(...(JSON.parse(eventsJson) as unknown[]))
      } catch {
        /* malformed notification payload — drop */
      }
      notify()
    },
    pushBell: () => {
      bellPending = true
      notify()
    },
    takeOscEvents: () => {
      if (oscEvents.length === 0) {
        return undefined
      }
      const json = JSON.stringify(oscEvents)
      oscEvents = []
      return json
    },
    takeNotifications: () => {
      if (pendingNotifications.length === 0) {
        return undefined
      }
      const json = JSON.stringify(pendingNotifications)
      pendingNotifications = []
      return json
    },
    drainBell: () => {
      const fired = bellPending
      bellPending = false
      return fired
    }
  }
}
