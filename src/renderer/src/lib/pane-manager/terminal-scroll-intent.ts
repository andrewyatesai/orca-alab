import type { IDisposable } from './aterm/terminal-types'

type TerminalScrollIntentKind = 'followOutput' | 'pinnedViewport'

type BufferType = 'normal' | 'alternate'

type TerminalScrollIntentTarget = {
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

type TerminalScrollIntentKey = string

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
const XTERM_SCROLL_INTENT_POINTER_TARGET_CLASSES = [
  'xterm-viewport',
  'xterm-scrollbar',
  'xterm-slider'
] as const
const XTERM_SCROLL_INTENT_POINTER_TARGET_SELECTOR = XTERM_SCROLL_INTENT_POINTER_TARGET_CLASSES.map(
  (className) => `.${className}`
).join(',')

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

function bindTerminalScrollIntentKey(
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

function isTerminalScrollIntentPointerTarget(target: EventTarget | null): target is Element {
  if (typeof Element === 'undefined' || !(target instanceof Element)) {
    return false
  }
  // xterm's custom scrollbar uses separate thumb/track nodes from the viewport.
  return target.closest(XTERM_SCROLL_INTENT_POINTER_TARGET_SELECTOR) !== null
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
  setTimeout(sync, 80)
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
  const kind =
    existing?.kind ??
    (isAtBottom(snapshot.viewportY, snapshot.baseY) ? 'followOutput' : 'pinnedViewport')
  // For a durable pin carry the STORED absolute line, not the live snapshot: during a
  // cold-restore replay the buffer is mid-rebuild, so snapshot.viewportY is a position
  // relative to the (shorter/regrowing) bottom. Enforcing that drifting value would
  // walk the pin off its content; the stored viewportY is the absolute line the engine
  // clamps against. Fresh pins (no existing intent) still use the live position.
  const viewportY = kind === 'pinnedViewport' && existing ? existing.viewportY : snapshot.viewportY
  return {
    kind,
    bufferType: snapshot.bufferType,
    viewportY
  }
}

export function enforceTerminalWriteScrollIntent(
  terminal: TerminalScrollIntentTarget,
  snapshot: TerminalScrollIntentWriteSnapshot | null
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
  const targetY = clampViewportY(snapshot.viewportY, current.baseY)
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
  // Resume / visibility restore: BYPASS the per-write equality guard. On the worker
  // path `current` is the last async STATE snapshot, which lags the just-resized /
  // scrolled engine — so current.viewportY can spuriously equal the target and the
  // guarded enforce would SKIP the corrective scroll, leaving a pinned viewport snapped
  // to the live bottom. Post the restore UNCONDITIONALLY; the engine clamps the
  // absolute line against its live origin + retained scrollback, so it reconciles
  // against real state regardless of snapshot lag. (Left the per-write enforce guarded:
  // it runs per output chunk and must not post a scroll + redraw every time.)
  if (existing.kind === 'followOutput') {
    safeScrollCall(() => terminal.scrollToBottom?.())
    return
  }
  safeScrollCall(() => terminal.scrollToLine?.(existing.viewportY))
}

export function attachTerminalScrollIntentTracking(
  terminal: TerminalScrollIntentTarget,
  host: HTMLElement,
  intentKey?: TerminalScrollIntentKey
): IDisposable {
  if (!bindTerminalScrollIntentKey(terminal, intentKey)) {
    syncTerminalScrollIntentFromViewport(terminal)
  }
  let pointerScrollActive = false

  const onWheel = (event: WheelEvent): void => {
    if (event.deltaY < 0) {
      markTerminalPinnedViewport(terminal)
      syncTerminalScrollIntentSoon(terminal, { preservePinnedAtBottom: true })
      return
    }
    syncTerminalScrollIntentSoon(terminal)
  }

  const onPointerDown = (event: PointerEvent): void => {
    pointerScrollActive = isTerminalScrollIntentPointerTarget(event.target)
  }

  const onPointerDone = (): void => {
    if (!pointerScrollActive) {
      return
    }
    pointerScrollActive = false
    syncTerminalScrollIntentFromViewport(terminal)
  }

  const onScroll = (): void => {
    if (pointerScrollActive) {
      syncTerminalScrollIntentFromViewport(terminal)
    }
  }

  host.addEventListener('wheel', onWheel, { capture: true, passive: true })
  host.addEventListener('pointerdown', onPointerDown, true)
  host.addEventListener('scroll', onScroll, true)
  globalThis.addEventListener?.('pointerup', onPointerDone, true)
  globalThis.addEventListener?.('pointercancel', onPointerDone, true)
  return {
    dispose: () => {
      host.removeEventListener('wheel', onWheel, true)
      host.removeEventListener('pointerdown', onPointerDown, true)
      host.removeEventListener('scroll', onScroll, true)
      globalThis.removeEventListener?.('pointerup', onPointerDone, true)
      globalThis.removeEventListener?.('pointercancel', onPointerDone, true)
    }
  }
}
