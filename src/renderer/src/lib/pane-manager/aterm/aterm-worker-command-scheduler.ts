// Worker QoS command scheduler (R4): the shared render worker hosts EVERY pane's
// engine and, before this, funnelled every pane's `process`/`draw`/predict* through
// ONE FIFO message pump with no priority. A background pane flooded with output would
// run its `processBytes` to completion ahead of the FOCUSED pane's queued keystroke
// echo, so a keystroke could wait seconds behind sibling redraws.
//
// This scheduler adds cross-pane QoS WITHOUT reordering any single pane's bytes:
//   • Interactive/cheap work — anything for the focused pane, and every non-`process`
//     command — runs SYNCHRONOUSLY on arrival (the fast path), so keystroke echo,
//     predict*, draw, resize, etc. are never queued behind a flood.
//   • A BACKGROUND pane's `process` is split into bounded sub-chunks and drained on a
//     yielding macrotask loop, time-sliced so it hands the event loop back to pending
//     interactive messages every few ms.
//   • Per-pane FIFO is preserved absolutely: once a pane has ANY deferred work, all of
//     its later commands queue behind it (a pane's `process` bytes and their ordering
//     never change). Only the order BETWEEN panes shifts.
//   • Background panes still make progress (round-robin across them; the focused pane's
//     transient backlog gets priority but drains to empty and is not re-fed by the fast
//     path), so nothing starves forever.

import type { AtermWorkerPaneRuntimeCommand } from './aterm-render-worker-protocol'

/** A pane runtime command as it arrives on the wire (paneId-stamped). */
export type PaneRuntimeCommand = AtermWorkerPaneRuntimeCommand & { paneId: number }

export type AtermWorkerCommandSchedulerDeps = {
  /** Run ONE fully-formed runtime command against its pane (the worker's synchronous
   *  dispatch). May throw a wasm error; the caller wraps drain runs in its crash guard
   *  so a deferred-chunk panic still retires the worker like a synchronous one. */
  execute: (command: PaneRuntimeCommand) => void
  /** Schedule a macrotask that first lets already-queued worker messages run (so a
   *  focused-pane keystroke posted meanwhile is enqueued + fast-pathed first), then
   *  resumes the drain. Production: a MessageChannel port; tests: a controllable stub. */
  scheduleDrain: (resume: () => void) => void
  /** Monotonic clock (ms) for the drain time-slice; default performance.now. */
  now?: () => number
  /** Max chars of a background pane's `process` handled per sub-chunk — bounds one
   *  drain unit's cost so a single huge frame can't blow the slice. */
  chunkChars?: number
  /** Wall-time (ms) one synchronous drain slice may spend before yielding to
   *  interactive work — bounds the worst-case focused-keystroke wait. */
  sliceMs?: number
}

export type AtermWorkerCommandScheduler = {
  /** Route ONE pane runtime command: fast-path interactive/cheap work, defer + chunk a
   *  background pane's bulk `process`. */
  submit: (command: PaneRuntimeCommand) => void
  /** Record a focus change (QoS priority). Only clears when the blurred pane is still
   *  the focused one, so a blur arriving after focus moved elsewhere is a no-op. */
  noteFocus: (paneId: number, focused: boolean) => void
  /** Drop a pane's deferred work (on dispose) so nothing runs against a freed engine. */
  forget: (paneId: number) => void
  /** Count of deferred units still queued (tests/introspection). */
  pendingCount: () => number
}

const DEFAULT_CHUNK_CHARS = 8192
const DEFAULT_SLICE_MS = 8

const defaultNow: () => number =
  typeof performance !== 'undefined' ? () => performance.now() : () => Date.now()

/** Split a string into <=chunkChars pieces WITHOUT severing a UTF-16 surrogate pair
 *  (a lone half would decode to U+FFFD in the wasm text encoder). Splitting mid escape
 *  sequence is safe — the engine's VTE parser is streaming and keeps state across
 *  process() calls, exactly as real PTY reads already arrive in arbitrary chunks. */
export function splitProcessData(data: string, chunkChars: number): string[] {
  if (data.length <= chunkChars) {
    return [data]
  }
  const chunks: string[] = []
  let i = 0
  while (i < data.length) {
    let end = Math.min(i + chunkChars, data.length)
    // Keep a trailing high surrogate glued to its low half in the next code unit.
    if (end < data.length) {
      const last = data.charCodeAt(end - 1)
      if (last >= 0xd800 && last <= 0xdbff) {
        end += 1
      }
    }
    chunks.push(data.slice(i, end))
    i = end
  }
  return chunks
}

export function createAtermWorkerCommandScheduler(
  deps: AtermWorkerCommandSchedulerDeps
): AtermWorkerCommandScheduler {
  const now = deps.now ?? defaultNow
  const chunkChars = deps.chunkChars ?? DEFAULT_CHUNK_CHARS
  const sliceMs = deps.sliceMs ?? DEFAULT_SLICE_MS

  // Per-pane FIFO of deferred commands. A present-but-non-empty entry is the "backlog"
  // that forces every later command for that pane to queue behind it.
  const queues = new Map<number, PaneRuntimeCommand[]>()
  // Round-robin order of panes with pending work (fairness across background floods).
  const ready: number[] = []
  let focusedPaneId: number | null = null
  let drainScheduled = false

  const enqueue = (paneId: number, item: PaneRuntimeCommand): void => {
    let q = queues.get(paneId)
    if (!q) {
      q = []
      queues.set(paneId, q)
      ready.push(paneId)
    }
    q.push(item)
  }

  const submit = (command: PaneRuntimeCommand): void => {
    const paneId = command.paneId
    const hasBacklog = (queues.get(paneId)?.length ?? 0) > 0
    // Only a BACKGROUND pane's `process` is bulk; everything else is interactive/cheap.
    const isBulk = command.type === 'process' && paneId !== focusedPaneId
    if (!hasBacklog && !isBulk) {
      deps.execute(command)
      return
    }
    if (command.type === 'process') {
      // Chunk here (cheap) so onmessage returns fast; the heavy parse happens in the
      // drain. Each sub-chunk is a self-contained `process` — side channels + repaint
      // fire per chunk exactly as if the app had sent several smaller writes.
      for (const data of splitProcessData(command.data, chunkChars)) {
        enqueue(paneId, { ...command, data })
      }
    } else {
      enqueue(paneId, command)
    }
    if (!drainScheduled) {
      drainScheduled = true
      deps.scheduleDrain(runSlice)
    }
  }

  const hasWork = (): boolean => ready.length > 0

  // Focused pane first (its transient backlog is what the user is watching), else the
  // round-robin head. Focused is not re-fed by the fast path, so it drains to empty and
  // hands the loop back to background panes.
  const pickPane = (): number | null => {
    if (focusedPaneId !== null && (queues.get(focusedPaneId)?.length ?? 0) > 0) {
      return focusedPaneId
    }
    return ready.length > 0 ? ready[0] : null
  }

  const servicePane = (paneId: number): void => {
    const q = queues.get(paneId)
    if (!q || q.length === 0) {
      return
    }
    const item = q.shift()
    if (q.length === 0) {
      queues.delete(paneId)
      const at = ready.indexOf(paneId)
      if (at !== -1) {
        ready.splice(at, 1)
      }
    } else if (ready[0] === paneId) {
      // Rotate the round-robin head so a huge backlog can't starve its siblings.
      ready.push(ready.shift() as number)
    }
    if (item) {
      deps.execute(item)
    }
  }

  // ONE synchronous drain slice: service units until the time budget is spent, then
  // yield so pending interactive messages (a focused keystroke) run before we resume.
  const runSlice = (): void => {
    drainScheduled = false
    const deadline = now() + sliceMs
    do {
      const paneId = pickPane()
      if (paneId === null) {
        break
      }
      servicePane(paneId)
    } while (hasWork() && now() < deadline)
    if (hasWork() && !drainScheduled) {
      drainScheduled = true
      deps.scheduleDrain(runSlice)
    }
  }

  return {
    submit,
    noteFocus: (paneId, focused) => {
      if (focused) {
        focusedPaneId = paneId
      } else if (focusedPaneId === paneId) {
        focusedPaneId = null
      }
    },
    forget: (paneId) => {
      queues.delete(paneId)
      const at = ready.indexOf(paneId)
      if (at !== -1) {
        ready.splice(at, 1)
      }
      if (focusedPaneId === paneId) {
        focusedPaneId = null
      }
    },
    pendingCount: () => {
      let total = 0
      for (const q of queues.values()) {
        total += q.length
      }
      return total
    }
  }
}
