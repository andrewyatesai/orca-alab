type TerminalScrollIntentKind = 'followOutput' | 'pinnedViewport'

type BufferType = 'normal' | 'alternate'

export type TerminalScrollIntentTarget = {
  buffer?: {
    active?: {
      type?: string
      viewportY?: number
      baseY?: number
    }
  }
  scrollToBottom?: () => void
  scrollToLine?: (line: number) => void
}

export type TerminalScrollIntentKey = string

type TerminalScrollIntent = {
  kind: TerminalScrollIntentKind
  bufferType: BufferType
  viewportY: number
  baseY: number
}

type TerminalScrollIntentWriteSnapshot = {
  kind: TerminalScrollIntentKind
  bufferType: BufferType
  viewportY: number
  baseY: number
}

type TerminalScrollIntentEnforceOptions = {
  // 'viewportLine' restores the absolute buffer line (correct while content
  // only grows). 'bottomOffset' restores the distance from the bottom —
  // required after a buffer rebuild (snapshot replay, reflow) renumbers rows.
  restoreBy?: 'viewportLine' | 'bottomOffset'
}

const terminalScrollIntentByTerminal = new WeakMap<
  TerminalScrollIntentTarget,
  TerminalScrollIntent
>()
const terminalScrollIntentKeyByTerminal = new WeakMap<
  TerminalScrollIntentTarget,
  TerminalScrollIntentKey
>()
const terminalScrollIntentByKey = new Map<TerminalScrollIntentKey, TerminalScrollIntent>()

const BOTTOM_TOLERANCE_ROWS = 1

// While > 0, intent WRITES are frozen: writeIntent returns the durable stored intent
// unchanged instead of recording the live (transient) buffer position. Held across a
// worktree-switch resume + its cold-restore replay flood, where the buffer is cleared
// and regrown — without this freeze a transient empty/regrowing buffer overwrites the
// durable ABSOLUTE pin (vY=121) with a position RELATIVE to the rebuilt bottom (vY=225
// = baseY-6), so the restore lands on the wrong content. enforce* may still SCROLL
// (re-anchor) while frozen; only the intent STORE is gated. Depth-counted so nested /
// concurrent resume windows compose, and always released on a bounded timer (see
// terminal-visibility-resume) so it can never get stuck on.
let scrollIntentWriteFreezeDepth = 0

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

function readBufferSnapshot(
  terminal: TerminalScrollIntentTarget
): { bufferType: BufferType; viewportY: number; baseY: number } | null {
  const buffer = terminal.buffer?.active
  const viewportY = buffer?.viewportY
  const baseY = buffer?.baseY
  if (typeof viewportY !== 'number' || typeof baseY !== 'number') {
    return null
  }
  return {
    bufferType: buffer?.type === 'alternate' ? 'alternate' : 'normal',
    viewportY,
    baseY
  }
}

function isAtBottom(viewportY: number, baseY: number): boolean {
  return viewportY >= baseY - BOTTOM_TOLERANCE_ROWS
}

function writeIntent(
  terminal: TerminalScrollIntentTarget,
  kind: TerminalScrollIntentKind
): TerminalScrollIntent | null {
  // Frozen during a resume/replay window: keep the durable absolute pin; do not let a
  // transient (cleared/regrowing) buffer re-store a relative position over it.
  if (scrollIntentWriteFreezeDepth > 0) {
    return readStoredIntent(terminal) ?? null
  }
  const snapshot = readBufferSnapshot(terminal)
  if (!snapshot) {
    return null
  }
  const intent = { kind, ...snapshot }
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
  const existing = terminalScrollIntentByKey.get(key)
  if (existing) {
    terminalScrollIntentByTerminal.set(terminal, existing)
  }
  return existing
}

function clampViewportY(viewportY: number, baseY: number): number {
  return Math.max(0, Math.min(viewportY, baseY))
}

function safeScrollCall(fn: () => void): boolean {
  try {
    fn()
    return true
  } catch (err) {
    if (err instanceof TypeError && /dimensions/.test(err.message)) {
      return false
    }
    throw err
  }
}

export function markTerminalFollowOutput(terminal: TerminalScrollIntentTarget): void {
  writeIntent(terminal, 'followOutput')
}

export function markTerminalPinnedViewport(terminal: TerminalScrollIntentTarget): void {
  writeIntent(terminal, 'pinnedViewport')
}

export function syncTerminalScrollIntentFromViewport(
  terminal: TerminalScrollIntentTarget,
  options: { preservePinnedAtBottom?: boolean } = {}
): void {
  const snapshot = readBufferSnapshot(terminal)
  if (!snapshot) {
    return
  }
  const existing = readStoredIntent(terminal)
  // Why: a remounted/replayed terminal can briefly report an empty or shorter
  // scrollback. That transient state must not erase a durable pinned viewport.
  if (existing?.kind === 'pinnedViewport' && snapshot.baseY < existing.baseY) {
    terminalScrollIntentByTerminal.set(terminal, existing)
    return
  }
  if (
    options.preservePinnedAtBottom &&
    existing?.kind === 'pinnedViewport' &&
    isAtBottom(snapshot.viewportY, snapshot.baseY)
  ) {
    return
  }
  writeIntent(
    terminal,
    isAtBottom(snapshot.viewportY, snapshot.baseY) ? 'followOutput' : 'pinnedViewport'
  )
}

export function syncTerminalScrollIntentSoon(
  terminal: TerminalScrollIntentTarget,
  options: { preservePinnedAtBottom?: boolean } = {}
): void {
  const sync = (): void => syncTerminalScrollIntentFromViewport(terminal, options)
  queueMicrotask(sync)
  requestAnimationFrame(sync)
  requestAnimationFrame(() => requestAnimationFrame(sync))
  // Why: preservePinnedAtBottom only bridges xterm's async scroll application.
  // The settle tick must reclassify from the real viewport, otherwise a wheel
  // the viewport never followed (sub-row delta, TUI-consumed mouse report,
  // plain PageUp/Home sent to the app) latches a phantom pin at the bottom.
  setTimeout(() => syncTerminalScrollIntentFromViewport(terminal), 80)
}

export function getTerminalScrollIntentKind(
  terminal: TerminalScrollIntentTarget
): TerminalScrollIntentKind {
  const existing = readStoredIntent(terminal)
  if (existing) {
    return existing.kind
  }
  const snapshot = readBufferSnapshot(terminal)
  if (!snapshot) {
    return 'followOutput'
  }
  return isAtBottom(snapshot.viewportY, snapshot.baseY) ? 'followOutput' : 'pinnedViewport'
}

export function captureTerminalWriteScrollIntent(
  terminal: TerminalScrollIntentTarget
): TerminalScrollIntentWriteSnapshot | null {
  const snapshot = readBufferSnapshot(terminal)
  if (!snapshot) {
    return null
  }
  const existing = readStoredIntent(terminal)
  let kind =
    existing?.kind ??
    (isAtBottom(snapshot.viewportY, snapshot.baseY) ? 'followOutput' : 'pinnedViewport')
  // Why: a pinned intent whose live viewport still sits at the bottom is a
  // phantom pin (the user's scroll never detached the viewport). Enforcing it
  // would freeze the terminal at the current line on every write batch (#8625).
  if (kind === 'pinnedViewport' && isAtBottom(snapshot.viewportY, snapshot.baseY)) {
    kind = 'followOutput'
  }
  return {
    kind,
    bufferType: snapshot.bufferType,
    viewportY: snapshot.viewportY,
    baseY: snapshot.baseY
  }
}

export function enforceTerminalWriteScrollIntent(
  terminal: TerminalScrollIntentTarget,
  snapshot: TerminalScrollIntentWriteSnapshot | null,
  options: TerminalScrollIntentEnforceOptions = {}
): void {
  if (!snapshot) {
    return
  }
  const current = readBufferSnapshot(terminal)
  if (!current || current.bufferType !== snapshot.bufferType) {
    return
  }
  if (snapshot.kind === 'followOutput') {
    if (safeScrollCall(() => terminal.scrollToBottom?.())) {
      writeIntent(terminal, 'followOutput')
    }
    return
  }
  const requestedY =
    options.restoreBy === 'bottomOffset'
      ? current.baseY - Math.max(0, snapshot.baseY - snapshot.viewportY)
      : snapshot.viewportY
  const targetY = clampViewportY(requestedY, current.baseY)
  if (current.viewportY !== targetY) {
    safeScrollCall(() => terminal.scrollToLine?.(targetY))
  }
  writeIntent(terminal, 'pinnedViewport')
}

export function enforceTerminalCurrentScrollIntent(terminal: TerminalScrollIntentTarget): void {
  const existing = readStoredIntent(terminal)
  if (!existing) {
    // Nothing durable pinned yet — capture + enforce from the live snapshot (the
    // per-write equality guard is fine when there is no stored target to restore).
    enforceTerminalWriteScrollIntent(terminal, captureTerminalWriteScrollIntent(terminal))
    return
  }
  const current = readBufferSnapshot(terminal)
  // Don't restore a normal-buffer pin into an alternate screen (TUI) or vice versa.
  if (current && current.bufferType !== existing.bufferType) {
    return
  }
  // Why: a pin recorded at the bottom means the viewport never detached (phantom
  // pin, #8625); resuming must follow live output, not freeze at that stale line.
  const kind =
    existing.kind === 'pinnedViewport' && isAtBottom(existing.viewportY, existing.baseY)
      ? 'followOutput'
      : existing.kind
  // Resume / visibility restore: BYPASS the per-write equality guard. On the worker
  // path `current` is the last async STATE snapshot, which lags the just-resized /
  // scrolled engine — so current.viewportY can spuriously equal the target and the
  // guarded enforce would SKIP the corrective scroll, leaving a pinned viewport snapped
  // to the live bottom. Post the restore UNCONDITIONALLY; the engine clamps the
  // absolute line against its live origin + retained scrollback, so it reconciles
  // against real state regardless of snapshot lag. (Left the per-write enforce guarded:
  // it runs per output chunk and must not post a scroll + redraw every time.)
  if (kind === 'followOutput') {
    // Persist a phantom-pin demotion (or an already-following intent) so the stored
    // intent tracks live output after this resume too, not just this scroll.
    if (safeScrollCall(() => terminal.scrollToBottom?.())) {
      writeIntent(terminal, 'followOutput')
    }
    return
  }
  // Why bottomOffset when the live buffer is shorter than the stored pin: a snapshot
  // replay/remount rebuilds and RENUMBERS absolute lines, so restoring the stored
  // absolute line would over-clamp; restore the distance-from-bottom instead (#8625).
  // When the buffer only grew, this reduces to the stored absolute line.
  const targetY =
    current && current.baseY < existing.baseY
      ? clampViewportY(
          current.baseY - Math.max(0, existing.baseY - existing.viewportY),
          current.baseY
        )
      : existing.viewportY
  safeScrollCall(() => terminal.scrollToLine?.(targetY))
}
