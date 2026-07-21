import { isTerminalScrollIntentRebuildInFlight } from './terminal-scroll-intent-rebuild'
import { areScrollIntentWritesFrozen } from './terminal-scroll-intent-write-freeze'
import {
  clampTerminalViewportY,
  isTerminalViewportAtBottom,
  readTerminalScrollBufferSnapshot,
  safeTerminalScrollCall,
  type TerminalScrollBufferType
} from './terminal-scroll-buffer-snapshot'
import type {
  TerminalScrollIntentEnforceOptions,
  TerminalScrollIntentKey,
  TerminalScrollIntentKind,
  TerminalScrollIntentTarget,
  TerminalStructuralScrollIntentSnapshot
} from './terminal-scroll-intent-types'

type TerminalScrollIntent = {
  kind: TerminalScrollIntentKind
  bufferType: TerminalScrollBufferType
  viewportY: number
  baseY: number
  revision: number
}

const terminalScrollIntentByTerminal = new WeakMap<
  TerminalScrollIntentTarget,
  TerminalScrollIntent
>()
const terminalScrollIntentKeyByTerminal = new WeakMap<
  TerminalScrollIntentTarget,
  TerminalScrollIntentKey
>()
const terminalScrollIntentKeyBindingByTerminal = new WeakMap<TerminalScrollIntentTarget, number>()
const terminalScrollIntentByKey = new Map<TerminalScrollIntentKey, TerminalScrollIntent>()
const terminalScrollIntentBindingByKey = new Map<TerminalScrollIntentKey, number>()

let nextTerminalScrollIntentRevision = 1
let nextTerminalScrollIntentKeyBinding = 1

// Re-exported (module moved for the max-lines split): resume callers and
// module mocks address the write freeze through terminal-scroll-intent.
export {
  beginSuppressScrollIntentWrites,
  endSuppressScrollIntentWrites,
  runWithSuppressedScrollIntentWrites
} from './terminal-scroll-intent-write-freeze'

function writeIntent(
  terminal: TerminalScrollIntentTarget,
  kind: TerminalScrollIntentKind
): TerminalScrollIntent | null {
  // Frozen during a resume/replay window: keep the durable absolute pin; do not let a
  // transient (cleared/regrowing) buffer re-store a relative position over it.
  if (areScrollIntentWritesFrozen()) {
    return readStoredIntent(terminal) ?? null
  }
  const snapshot = readTerminalScrollBufferSnapshot(terminal)
  if (!snapshot) {
    return null
  }
  return writeIntentSnapshot(terminal, kind, snapshot)
}

function writeIntentSnapshot(
  terminal: TerminalScrollIntentTarget,
  kind: TerminalScrollIntentKind,
  snapshot: { bufferType: TerminalScrollBufferType; viewportY: number; baseY: number }
): TerminalScrollIntent {
  const intent = { kind, ...snapshot, revision: nextTerminalScrollIntentRevision }
  nextTerminalScrollIntentRevision += 1
  terminalScrollIntentByTerminal.set(terminal, intent)
  const key = terminalScrollIntentKeyByTerminal.get(terminal)
  if (key) {
    terminalScrollIntentByKey.set(key, intent)
  }
  return intent
}

function readStoredIntent(terminal: TerminalScrollIntentTarget): TerminalScrollIntent | undefined {
  const terminalIntent = terminalScrollIntentByTerminal.get(terminal)
  if (terminalIntent) {
    return terminalIntent
  }
  const key = terminalScrollIntentKeyByTerminal.get(terminal)
  return key ? terminalScrollIntentByKey.get(key) : undefined
}

export function bindTerminalScrollIntentKey(
  terminal: TerminalScrollIntentTarget,
  key: TerminalScrollIntentKey | undefined
): TerminalScrollIntent | undefined {
  if (!key) {
    return terminalScrollIntentByTerminal.get(terminal)
  }
  terminalScrollIntentKeyByTerminal.set(terminal, key)
  const binding = nextTerminalScrollIntentKeyBinding
  nextTerminalScrollIntentKeyBinding += 1
  terminalScrollIntentKeyBindingByTerminal.set(terminal, binding)
  terminalScrollIntentBindingByKey.set(key, binding)
  const existing = terminalScrollIntentByKey.get(key)
  if (existing) {
    terminalScrollIntentByTerminal.set(terminal, existing)
  }
  return existing
}

export function isTerminalScrollIntentKeyBindingCurrent(
  terminal: TerminalScrollIntentTarget
): boolean {
  const key = terminalScrollIntentKeyByTerminal.get(terminal)
  if (!key) {
    return true
  }
  return (
    terminalScrollIntentKeyBindingByTerminal.get(terminal) ===
    terminalScrollIntentBindingByKey.get(key)
  )
}

export function markTerminalFollowOutput(terminal: TerminalScrollIntentTarget): void {
  writeIntent(terminal, 'followOutput')
}

export function markTerminalPinnedViewport(terminal: TerminalScrollIntentTarget): void {
  writeIntent(terminal, 'pinnedViewport')
}

/** Record an explicit user scroll even while visibility resume temporarily freezes
 *  background/system intent samples. Rebuild-in-flight remains authoritative: its
 *  transient buffer cannot replace a durable pre-replay pin. */
export function markTerminalPinnedViewportFromUserInteraction(
  terminal: TerminalScrollIntentTarget
): void {
  if (isTerminalScrollIntentRebuildInFlight(terminal)) {
    return
  }
  const snapshot = readTerminalScrollBufferSnapshot(terminal)
  if (snapshot) {
    writeIntentSnapshot(terminal, 'pinnedViewport', snapshot)
  }
}

export function syncTerminalScrollIntentFromViewport(
  terminal: TerminalScrollIntentTarget,
  options: {
    allowBufferShrink?: boolean
    preservePinnedAtBottom?: boolean
    /** A real wheel/scrollbar/key action may supersede a resume-window freeze. */
    userInteraction?: boolean
  } = {}
): void {
  if (isTerminalScrollIntentRebuildInFlight(terminal)) {
    return
  }
  const snapshot = readTerminalScrollBufferSnapshot(terminal)
  if (!snapshot) {
    return
  }
  const existing = readStoredIntent(terminal)
  // Why: a remounted/replayed terminal can briefly report an empty or shorter
  // scrollback. That transient state must not erase a durable pinned viewport.
  if (
    !options.allowBufferShrink &&
    existing?.kind === 'pinnedViewport' &&
    snapshot.baseY < existing.baseY
  ) {
    terminalScrollIntentByTerminal.set(terminal, existing)
    return
  }
  if (
    options.preservePinnedAtBottom &&
    existing?.kind === 'pinnedViewport' &&
    isTerminalViewportAtBottom(snapshot.viewportY, snapshot.baseY)
  ) {
    return
  }
  const kind = isTerminalViewportAtBottom(snapshot.viewportY, snapshot.baseY)
    ? 'followOutput'
    : 'pinnedViewport'
  // Why: parser auto-replies and repeated wheel settle samples often observe
  // no intent change. Avoid manufacturing revisions that can cancel a valid
  // structural restore or amplify terminal-output bursts.
  if (
    existing?.kind === kind &&
    existing.bufferType === snapshot.bufferType &&
    (kind === 'followOutput' || existing.viewportY === snapshot.viewportY)
  ) {
    if (kind === 'pinnedViewport' && existing.baseY !== snapshot.baseY) {
      // Why: native pinned output can grow baseY without moving viewportY.
      // Refresh geometry without creating a user-intent revision so a later
      // keyed remount restores the same content, not the stale bottom offset.
      Object.assign(existing, snapshot)
    }
    return
  }
  if (options.userInteraction) {
    writeIntentSnapshot(terminal, kind, snapshot)
  } else {
    writeIntent(terminal, kind)
  }
}

export function getTerminalScrollIntentKind(
  terminal: TerminalScrollIntentTarget
): TerminalScrollIntentKind {
  const existing = readStoredIntent(terminal)
  if (existing) {
    return existing.kind
  }
  const snapshot = readTerminalScrollBufferSnapshot(terminal)
  if (!snapshot) {
    return 'followOutput'
  }
  return isTerminalViewportAtBottom(snapshot.viewportY, snapshot.baseY)
    ? 'followOutput'
    : 'pinnedViewport'
}

export function captureTerminalStructuralScrollIntent(
  terminal: TerminalScrollIntentTarget
): TerminalStructuralScrollIntentSnapshot | null {
  if (isTerminalScrollIntentRebuildInFlight(terminal)) {
    return null
  }
  const snapshot = readTerminalScrollBufferSnapshot(terminal)
  if (!snapshot) {
    return null
  }
  const existing = readStoredIntent(terminal)
  let kind =
    existing?.kind ??
    (isTerminalViewportAtBottom(snapshot.viewportY, snapshot.baseY)
      ? 'followOutput'
      : 'pinnedViewport')
  // Why: a pinned intent whose live viewport still sits at the bottom is a
  // phantom pin (the user's scroll never detached the viewport). Restoring it
  // after a structural operation would freeze the terminal at a stale line.
  // Only trust the at-bottom reading when the scrollback is at least as long
  // as the pin's — a shorter one is a cleared buffer awaiting replay.
  if (
    kind === 'pinnedViewport' &&
    isTerminalViewportAtBottom(snapshot.viewportY, snapshot.baseY) &&
    (!existing || snapshot.baseY >= existing.baseY)
  ) {
    kind = 'followOutput'
  }
  // Why: a keyed remount starts at 0/0 before replay. Preserve the durable
  // pre-remount coordinates or a bottom-offset restore silently loses the pin.
  const capturedCoordinates =
    existing?.kind === 'pinnedViewport' && snapshot.baseY < existing.baseY ? existing : snapshot
  return {
    ...capturedCoordinates,
    kind,
    revision: existing?.revision ?? 0
  }
}

export function isTerminalStructuralScrollIntentCurrent(
  terminal: TerminalScrollIntentTarget,
  snapshot: TerminalStructuralScrollIntentSnapshot | null
): boolean {
  if (!snapshot) {
    return false
  }
  return (readStoredIntent(terminal)?.revision ?? 0) === snapshot.revision
}

export function restoreTerminalStructuralScrollIntent(
  terminal: TerminalScrollIntentTarget,
  snapshot: TerminalStructuralScrollIntentSnapshot | null,
  options: TerminalScrollIntentEnforceOptions = {}
): void {
  if (
    !snapshot ||
    !isTerminalStructuralScrollIntentCurrent(terminal, snapshot) ||
    isTerminalScrollIntentRebuildInFlight(terminal)
  ) {
    return
  }
  const current = readTerminalScrollBufferSnapshot(terminal)
  if (!current || current.bufferType !== snapshot.bufferType) {
    return
  }
  if (snapshot.kind === 'followOutput') {
    if (safeTerminalScrollCall(() => terminal.scrollToBottom?.())) {
      writeIntent(terminal, 'followOutput')
    }
    return
  }
  const requestedY =
    options.restoreBy === 'bottomOffset'
      ? current.baseY - Math.max(0, snapshot.baseY - snapshot.viewportY)
      : snapshot.viewportY
  const targetY = clampTerminalViewportY(requestedY, current.baseY)
  if (options.forceScroll === true || current.viewportY !== targetY) {
    if (!safeTerminalScrollCall(() => terminal.scrollToLine?.(targetY))) {
      // Why: renderer teardown can reject the scroll before xterm changes its
      // native viewport; retain the intended pin for the next fit/retry rather
      // than latching the transient current bottom.
      writeIntentSnapshot(terminal, 'pinnedViewport', {
        bufferType: current.bufferType,
        viewportY: targetY,
        baseY: current.baseY
      })
      return
    }
  }
  const existing = readStoredIntent(terminal)
  // Why: a scrollback shorter than the stored pin means the buffer is being
  // rebuilt; re-latching from it would overwrite the durable line with the
  // cleared buffer's line 0.
  if (existing?.kind === 'pinnedViewport' && current.baseY < existing.baseY) {
    return
  }
  writeIntent(terminal, 'pinnedViewport')
}

export function enforceTerminalCurrentScrollIntent(terminal: TerminalScrollIntentTarget): void {
  if (isTerminalScrollIntentRebuildInFlight(terminal)) {
    return
  }
  const existing = readStoredIntent(terminal)
  if (!existing) {
    restoreTerminalStructuralScrollIntent(terminal, captureTerminalStructuralScrollIntent(terminal))
    return
  }
  const snapshot = {
    kind: existing.kind,
    bufferType: existing.bufferType,
    viewportY: existing.viewportY,
    baseY: existing.baseY,
    revision: existing.revision
  }
  if (
    snapshot.kind === 'pinnedViewport' &&
    isTerminalViewportAtBottom(snapshot.viewportY, snapshot.baseY)
  ) {
    // Why: a pin recorded at the bottom means the viewport never detached;
    // resuming must follow live output, not freeze at that stale line.
    snapshot.kind = 'followOutput'
  }
  const current = readTerminalScrollBufferSnapshot(terminal)
  // Why: a shorter live buffer than the stored intent means the buffer was
  // rebuilt (snapshot replay/remount); absolute lines are renumbered there.
  const restoreBy =
    snapshot.kind === 'pinnedViewport' && current && current.baseY < snapshot.baseY
      ? 'bottomOffset'
      : 'viewportLine'
  // forceScroll (fork, worker path): resume-style enforcement must post the
  // corrective scroll even when the lagging facade snapshot claims the viewport
  // already matches — the engine reconciles against its live state.
  restoreTerminalStructuralScrollIntent(terminal, snapshot, { restoreBy, forceScroll: true })
}
