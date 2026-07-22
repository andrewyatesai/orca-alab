import type { ManagedPane, PaneManager } from '@/lib/pane-manager/pane-manager'
import { isTerminalInstanceDisposed } from '@/lib/pane-manager/terminal-instance-disposed'
import { getTerminalWriteGeneration } from '@/lib/pane-manager/terminal-write-generation'
import { trimTrailingAltScreenEnter, POST_REPLAY_MODE_RESET } from './layout-serialization'
import { replayIntoTerminal, type ReplayingPanesRef } from './replay-guard'

// P5: the mount-time restore replays only the sync 512KB tail (fast first
// paint). This module then streams the OLDER portion of the 5MB snapshot store
// from main in bounded async chunks and — only while nothing else has written
// to the terminal — rebuilds the pane as clear + full history, so the user gets
// up to ~10x more restored scrollback without a synchronous 5MB read or replay.

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
// Why sliced: keeps each engine write/postMessage bounded; all slices enqueue in
// ONE synchronous turn so the byte stream stays contiguous (split escapes are fine).
const REPLAY_SLICE_CHARS = 1024 * 1024

/**
 * Streams the pre-tail snapshot region and atomically rebuilds the pane with the
 * full history. Aborts silently — leaving the already-painted tail restore as-is —
 * when the pane is disposed/cancelled, the snapshot changed on disk, or any other
 * writer (live PTY bytes, structural reattach/cold-restore replay) touched the
 * terminal since the tail replay. Returns a cancel function for unmount.
 */
export function startTerminalScrollbackDeepRestore(args: {
  pane: ManagedPane
  manager: PaneManager
  source: TerminalScrollbackDeepRestoreSource
  replayingPanesRef: ReplayingPanesRef
  readOlderChunk: TerminalScrollbackOlderChunkReader
}): () => void {
  const { pane, manager, source, replayingPanesRef, readOlderChunk } = args
  let cancelled = false
  // Captured in the same synchronous mount turn as the tail replay, so the first
  // foreign write after it flips the generation and vetoes the rebuild.
  const fenceGeneration = getTerminalWriteGeneration(pane.terminal)

  const abortedByOtherWriter = (): boolean =>
    cancelled ||
    isTerminalInstanceDisposed(pane.terminal) ||
    getTerminalWriteGeneration(pane.terminal) !== fenceGeneration

  void (async () => {
    const olderChunks: string[] = []
    let cursor = source.olderChunkCursor
    while (cursor < source.olderEndOffset) {
      // Why check per chunk: once live output lands the rebuild is dead — stop paying for reads.
      if (abortedByOtherWriter()) {
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
    if (olderChunks.length === 0 || abortedByOtherWriter()) {
      return
    }
    const full = trimTrailingAltScreenEnter(olderChunks.join('') + source.tailText)
    if (full.length === 0) {
      return
    }
    const renderOptions = {
      shouldRefreshViewportSynchronously: () => !manager.hasWebglRenderer(pane.id)
    }
    // From here everything enqueues in one synchronous turn: writes are FIFO, so
    // later live output lands after the rebuilt history instead of inside it.
    replayIntoTerminal(pane, replayingPanesRef, DEEP_RESTORE_CLEAR, renderOptions)
    for (let i = 0; i < full.length; i += REPLAY_SLICE_CHARS) {
      replayIntoTerminal(pane, replayingPanesRef, full.slice(i, i + REPLAY_SLICE_CHARS), renderOptions)
    }
    // Same epilogue as the mount tail replay: prompt-safe newline + mode reset.
    replayIntoTerminal(pane, replayingPanesRef, '\r\n', renderOptions)
    replayIntoTerminal(pane, replayingPanesRef, POST_REPLAY_MODE_RESET, renderOptions)
  })()

  return () => {
    cancelled = true
  }
}
