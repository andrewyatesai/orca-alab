import type { ManagedPane, PaneManager } from '@/lib/pane-manager/pane-manager'
import { isTerminalInstanceDisposed } from '@/lib/pane-manager/terminal-instance-disposed'
import { getTerminalWriteGeneration } from '@/lib/pane-manager/terminal-write-generation'
import { POST_REPLAY_MODE_RESET } from './layout-serialization'
import { replayIntoTerminal, type ReplayingPanesRef } from './replay-guard'

// P5: the mount-time restore replays only the sync 512KB tail (fast first
// paint). This module then streams the OLDER portion of the 5MB snapshot store
// from main in bounded async chunks and — only while nothing else has written
// to the terminal — rebuilds the pane as clear + full history. The rebuild is
// itself sliced across macrotask turns: each engine feed stays bounded so a
// 5MB replay never runs as one synchronous renderer-blocking turn.

export type TerminalScrollbackDeepRestoreSource = {
  ref: string
  /** The tail the mount replay painted; the rebuild replays older + this. */
  tailText: string
  olderChunkCursor: number
  olderEndOffset: number
  fingerprint: string
}

export type TerminalScrollbackOlderChunkReader = (args: {
  ref: string
  cursor: number
  endOffset: number
  fingerprint: string
}) => Promise<{ text: string; nextCursor: number } | null>

// 2J+3J+H: wipe the tail-only paint (viewport AND its scrollback) so the full replay is the sole content.
const DEEP_RESTORE_CLEAR = '\x1b[2J\x1b[3J\x1b[H'
// Why 256KB: one engine feed + one xterm write per turn; bounded so each
// hydration turn costs a few ms and input/paint/scheduler drains interleave.
export const DEEP_RESTORE_REPLAY_SLICE_CHARS = 256 * 1024

const ALT_SCREEN_ENTER = '\x1b[?1049h'
const ALT_SCREEN_LEAVE = '\x1b[?1049l'

/** Same cut as trimTrailingAltScreenEnter over the pieces' concatenation, found
 *  by scanning with a small overlap carry (a sequence can straddle a chunk
 *  boundary) so the multi-MB joined string is never materialized. Returns the
 *  global char count to replay. */
export function altScreenReplayEndOffset(pieces: readonly string[]): number {
  const overlap = ALT_SCREEN_ENTER.length - 1
  let lastEnter = -1
  let lastLeave = -1
  let base = 0
  let carry = ''
  for (const piece of pieces) {
    const scan = carry + piece
    const scanStart = base - carry.length
    const enter = scan.lastIndexOf(ALT_SCREEN_ENTER)
    // Why unconditional assign: scans move strictly forward, so a hit here is always the latest.
    if (enter !== -1) {
      lastEnter = scanStart + enter
    }
    const leave = scan.lastIndexOf(ALT_SCREEN_LEAVE)
    if (leave !== -1) {
      lastLeave = scanStart + leave
    }
    base += piece.length
    carry = scan.slice(-overlap)
  }
  return lastEnter > lastLeave ? lastEnter : base
}

// Why MessageChannel: Chromium clamps nested setTimeout(0) to ~4ms; a posted
// macrotask isn't clamped yet still yields to input/paint/scheduler drains
// between slices (same pattern as the output scheduler's drain channel).
function yieldToRendererTasks(): Promise<void> {
  return new Promise((resolve) => {
    if (typeof MessageChannel === 'undefined') {
      setTimeout(resolve, 0)
      return
    }
    const channel = new MessageChannel()
    channel.port1.onmessage = () => {
      channel.port1.close()
      channel.port2.close()
      resolve()
    }
    channel.port2.postMessage(undefined)
  })
}

/**
 * Streams the pre-tail snapshot region and rebuilds the pane with the full
 * history, one bounded slice per macrotask turn. Aborts silently — leaving the
 * already-painted tail restore as-is — when the pane is disposed/cancelled, the
 * snapshot changed on disk, or any other writer (live PTY bytes, structural
 * reattach/cold-restore replay) touched the terminal before the rebuild's first
 * write. A foreign write landing BETWEEN slices stops the rebuild too, followed
 * by one mode reset (see below). Returns a cancel function for unmount.
 */
export function startTerminalScrollbackDeepRestore(args: {
  pane: ManagedPane
  manager: PaneManager
  source: TerminalScrollbackDeepRestoreSource
  replayingPanesRef: ReplayingPanesRef
  readOlderChunk: TerminalScrollbackOlderChunkReader
  /** Test seam; production always yields a real macrotask between slices. */
  yieldBetweenSlices?: () => Promise<void>
}): () => void {
  const { pane, manager, source, replayingPanesRef, readOlderChunk } = args
  const yieldBetweenSlices = args.yieldBetweenSlices ?? yieldToRendererTasks
  let cancelled = false
  // Captured in the same synchronous mount turn as the tail replay; re-armed
  // after each of our own writes, so any FOREIGN write flips it (both output
  // funnels bump at enqueue — before their bytes can reach the engine).
  let fence = getTerminalWriteGeneration(pane.terminal)

  const cancelledOrDisposed = (): boolean => cancelled || isTerminalInstanceDisposed(pane.terminal)
  const foreignWriterLanded = (): boolean => getTerminalWriteGeneration(pane.terminal) !== fence

  const renderOptions = {
    shouldRefreshViewportSynchronously: () => !manager.hasWebglRenderer(pane.id)
  }
  const writeReplaySlice = (data: string): void => {
    replayIntoTerminal(pane, replayingPanesRef, data, renderOptions)
    // Our own write bumped the generation; re-arm in the same sync turn so only
    // OTHER writers can flip the fence before the next slice.
    fence = getTerminalWriteGeneration(pane.terminal)
  }

  void (async () => {
    // Phase 1: read the whole older region BEFORE the first write, so a read
    // failure or on-disk snapshot rewrite aborts with the tail paint untouched.
    const olderChunks: string[] = []
    let cursor = source.olderChunkCursor
    while (cursor < source.olderEndOffset) {
      // Why check per chunk: once live output lands the rebuild is dead — stop paying for reads.
      if (cancelledOrDisposed() || foreignWriterLanded()) {
        return
      }
      let chunk: Awaited<ReturnType<TerminalScrollbackOlderChunkReader>>
      try {
        chunk = await readOlderChunk({
          ref: source.ref,
          cursor,
          endOffset: source.olderEndOffset,
          fingerprint: source.fingerprint
        })
      } catch {
        return
      }
      // Null = snapshot changed on disk (fingerprint) or no forward progress: abort, keep the tail.
      if (!chunk || chunk.nextCursor <= cursor) {
        return
      }
      olderChunks.push(chunk.text)
      cursor = chunk.nextCursor
    }
    if (olderChunks.length === 0 || cancelledOrDisposed() || foreignWriterLanded()) {
      return
    }
    const pieces = [...olderChunks, source.tailText]
    const replayEnd = altScreenReplayEndOffset(pieces)
    if (replayEnd === 0) {
      return
    }

    // Phase 2: sliced replay — one bounded engine feed per macrotask turn, in
    // strict oldest-to-newest order. The first turn runs synchronously off the
    // last read (fence still clean), so clear + first slice are atomic with it.
    let first = true
    let offset = 0
    for (const piece of pieces) {
      const pieceEnd = Math.min(piece.length, replayEnd - offset)
      let i = 0
      while (i < pieceEnd) {
        const slice = piece.slice(i, Math.min(i + DEEP_RESTORE_REPLAY_SLICE_CHARS, pieceEnd))
        if (first) {
          writeReplaySlice(DEEP_RESTORE_CLEAR)
          first = false
        } else {
          await yieldBetweenSlices()
          if (cancelledOrDisposed()) {
            return
          }
          if (foreignWriterLanded()) {
            // Live bytes are now interleaved mid-history; stop rebuilding. Still
            // reset modes: a partial replay can end between a recorded TUI's
            // mode-arm and its disarm, which would silently break mouse/keys.
            writeReplaySlice(POST_REPLAY_MODE_RESET)
            return
          }
        }
        writeReplaySlice(slice)
        i += slice.length
      }
      offset += piece.length
      if (offset >= replayEnd) {
        break
      }
    }
    if (first) {
      return
    }
    // Same epilogue as the mount tail replay: prompt-safe newline + mode reset.
    writeReplaySlice('\r\n')
    writeReplaySlice(POST_REPLAY_MODE_RESET)
  })()

  return () => {
    cancelled = true
  }
}
