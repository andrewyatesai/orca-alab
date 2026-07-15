import {
  bufferPreHandlerPtyExit,
  clearPreHandlerPtyState,
  consumePreHandlerPtyState
} from './pty-pre-handler-buffer'
import { recordRecentPtyExit } from './pty-recent-exit-tracker'

type PtyExitDelivery = {
  ptyId: string
  code: number
  primary?: (code: number) => void
  sidecars: readonly ((code: number, context: { hadPrimary: boolean }) => void)[]
}

/** Delivers one exit to its primary owner and every observational sidecar.
 *  Every sidecar runs even if the primary (or an earlier sidecar) throws; the
 *  first error is rethrown after the full fanout so an owner's cleanup failure
 *  cannot silently swallow the exit or strand the remaining observers. */
export function deliverPtyExitToHandlers(delivery: PtyExitDelivery): void {
  const hadPrimary = delivery.primary !== undefined
  let firstError: unknown
  let hasError = false
  try {
    if (delivery.primary) {
      clearPreHandlerPtyState(delivery.ptyId)
      try {
        delivery.primary(delivery.code)
      } finally {
        // Why: ownership is final even when cleanup throws; a duplicate exit
        // must not become a new pre-handler event for a future mount.
        consumePreHandlerPtyState(delivery.ptyId)
      }
    } else {
      bufferPreHandlerPtyExit(delivery.ptyId, delivery.code)
    }
  } catch (error) {
    firstError = error
    hasError = true
  }

  // Why record here — after the primary cleared the pre-handler buffer, before
  // the sidecar fanout: a watcher that subscribes in this same tick (or later)
  // is replayed from the recent-exit tracker even though the buffer is gone. It
  // runs regardless of a primary throw so late subscribers never miss the exit.
  recordRecentPtyExit(delivery.ptyId, delivery.code, hadPrimary)

  for (const sidecar of delivery.sidecars) {
    try {
      sidecar(delivery.code, { hadPrimary })
    } catch (error) {
      if (!hasError) {
        firstError = error
        hasError = true
      }
    }
  }
  if (hasError) {
    throw firstError
  }
}
