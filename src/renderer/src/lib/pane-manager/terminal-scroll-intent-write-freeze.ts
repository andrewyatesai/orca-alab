// While > 0, scroll-intent WRITES are frozen: writeIntent returns the durable
// stored intent unchanged instead of recording the live (transient) buffer
// position. Held across a worktree-switch resume + its cold-restore replay
// flood, where the buffer is cleared and regrown — without this freeze a
// transient empty/regrowing buffer overwrites the durable ABSOLUTE pin with a
// position RELATIVE to the rebuilt bottom, so the restore lands on the wrong
// content. enforce* may still SCROLL (re-anchor) while frozen; only the intent
// STORE is gated. Depth-counted so nested/concurrent resume windows compose,
// and always released on a bounded timer (see terminal-visibility-resume) so it
// can never get stuck on. Complements the rebuild-in-flight gating: the freeze
// covers the visibility-resume window, which is not a coordinator-run
// structural replay.
let scrollIntentWriteFreezeDepth = 0

/** True while a resume/replay window holds the intent-store freeze. */
export function areScrollIntentWritesFrozen(): boolean {
  return scrollIntentWriteFreezeDepth > 0
}

/** Freeze intent writes (see `scrollIntentWriteFreezeDepth`). MUST be paired with
 *  `endSuppressScrollIntentWrites` — callers spanning async ticks release it on a
 *  bounded timer so a thrown resume body cannot strand the freeze on. */
export function beginSuppressScrollIntentWrites(): void {
  scrollIntentWriteFreezeDepth += 1
}

/** Release one freeze level (floored at 0 so a double-release is harmless). */
export function endSuppressScrollIntentWrites(): void {
  scrollIntentWriteFreezeDepth = Math.max(0, scrollIntentWriteFreezeDepth - 1)
}

/** Run `fn` with intent writes frozen, releasing on return/throw (synchronous use). */
export function runWithSuppressedScrollIntentWrites<T>(fn: () => T): T {
  beginSuppressScrollIntentWrites()
  try {
    return fn()
  } finally {
    endSuppressScrollIntentWrites()
  }
}
