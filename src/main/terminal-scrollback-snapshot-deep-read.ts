import { closeSync, openSync, readSync, statSync } from 'node:fs'
import {
  readTrailingUtf8,
  snapshotReadPaths,
  type TerminalScrollbackSnapshotStorage
} from './terminal-scrollback-snapshots'
import {
  TERMINAL_SCROLLBACK_DEEP_REPLAY_BYTE_LIMIT,
  TERMINAL_SCROLLBACK_OLDER_CHUNK_BYTE_LIMIT,
  TERMINAL_SCROLLBACK_REPLAY_BYTE_LIMIT
} from '../shared/terminal-scrollback-limits'

// P5 deep-restore reads: the mount-time restore stays a bounded sync tail read;
// the older portion of the (up to 5MB) snapshot store is then streamed to the
// renderer in bounded, UTF-8-aligned chunks — never one synchronous 5MB read.

export type TerminalScrollbackSnapshotTailRead = {
  /** Trailing bytes of the snapshot, UTF-8 aligned — replayed synchronously at mount. */
  text: string
  /** First byte of the older (pre-tail) region still on disk; equals olderEndOffset when nothing older is stored. */
  olderChunkCursor: number
  /** Exclusive end of the older region — exactly where `text` begins in the file. */
  olderEndOffset: number
  /** size:mtime consistency token; older-chunk reads bail when it no longer matches. */
  fingerprint: string
}

function snapshotFingerprint(size: number, mtimeMs: number): string {
  return `${size}:${mtimeMs}`
}

function deepRestoreOlderStartOffset(fileSize: number): number {
  return Math.max(0, fileSize - TERMINAL_SCROLLBACK_DEEP_REPLAY_BYTE_LIMIT)
}

/** Sync tail read (bounded by TERMINAL_SCROLLBACK_REPLAY_BYTE_LIMIT, as before)
 *  plus the offsets the async deep-restore needs to stream the older region. */
export function readTerminalScrollbackSnapshotTailSync(
  ref: string,
  storage?: TerminalScrollbackSnapshotStorage
): TerminalScrollbackSnapshotTailRead | null {
  for (const path of snapshotReadPaths(ref, storage)) {
    try {
      const stat = statSync(path)
      const text = readTrailingUtf8(path, TERMINAL_SCROLLBACK_REPLAY_BYTE_LIMIT)
      // Why byteLength: readTrailingUtf8 trims leading continuation bytes, so the
      // decoded text's UTF-8 length IS the exact file span it covers.
      const olderEndOffset = Math.max(0, stat.size - Buffer.byteLength(text, 'utf-8'))
      return {
        text,
        olderChunkCursor: deepRestoreOlderStartOffset(stat.size),
        olderEndOffset,
        fingerprint: snapshotFingerprint(stat.size, stat.mtimeMs)
      }
    } catch {
      // Try the legacy/global fallback when a profile-local snapshot is absent.
    }
  }
  return null
}

/** Bytes a trailing incomplete UTF-8 sequence occupies at the end of `bytes` (0-3). */
function trailingIncompleteUtf8Bytes(bytes: Buffer): number {
  for (let back = 1; back <= 3 && back <= bytes.length; back++) {
    const b = bytes[bytes.length - back]
    if ((b & 0xc0) !== 0x80) {
      // Found the lead byte of the final sequence; incomplete iff its declared length overruns the buffer.
      const expected =
        (b & 0xf8) === 0xf0 ? 4 : (b & 0xf0) === 0xe0 ? 3 : (b & 0xe0) === 0xc0 ? 2 : 1
      return expected > back ? back : 0
    }
  }
  return 0
}

/**
 * One bounded read of the older (pre-tail) snapshot region for async deep
 * hydration. Chunk boundaries stay UTF-8 aligned: a trailing split codepoint is
 * left for the next read (nextCursor backs up over it). Returns null when the
 * snapshot changed since the tail read (fingerprint mismatch), the args are out
 * of range, or no forward progress is possible — callers abort deep restore.
 */
export function readTerminalScrollbackSnapshotOlderChunkSync(
  args: {
    ref: string
    cursor: number
    endOffset: number
    fingerprint: string
  },
  storage?: TerminalScrollbackSnapshotStorage
): { text: string; nextCursor: number } | null {
  const { ref, cursor, endOffset, fingerprint } = args
  if (
    !Number.isSafeInteger(cursor) ||
    !Number.isSafeInteger(endOffset) ||
    cursor < 0 ||
    cursor >= endOffset
  ) {
    return null
  }
  for (const path of snapshotReadPaths(ref, storage)) {
    let fd: number
    let size: number
    try {
      const stat = statSync(path)
      // Why fingerprint over both roots: the tail may have come from the fallback root; only the matching file is byte-consistent.
      if (snapshotFingerprint(stat.size, stat.mtimeMs) !== fingerprint || endOffset > stat.size) {
        continue
      }
      size = stat.size
      fd = openSync(path, 'r')
    } catch {
      continue
    }
    try {
      const length = Math.min(TERMINAL_SCROLLBACK_OLDER_CHUNK_BYTE_LIMIT, endOffset - cursor)
      const bytes = Buffer.allocUnsafe(length)
      // Why bail on a short read: an atomic-rename rewrite can swap the inode between stat and open; a partial read means inconsistent bytes.
      if (readSync(fd, bytes, 0, length, cursor) !== length) {
        return null
      }
      // Align the region start like readTrailingUtf8: a codepoint straddling the
      // deep-limit boundary belongs to bytes we no longer retain.
      let start = 0
      if (cursor === deepRestoreOlderStartOffset(size)) {
        while (start < bytes.length && (bytes[start] & 0xc0) === 0x80) {
          start++
        }
      }
      const chunkEnd = cursor + length
      const trailingSplit = chunkEnd < endOffset ? trailingIncompleteUtf8Bytes(bytes) : 0
      const nextCursor = chunkEnd - trailingSplit
      if (nextCursor <= cursor) {
        return null
      }
      return {
        text: bytes.subarray(start, length - trailingSplit).toString('utf-8'),
        nextCursor
      }
    } catch {
      // Fall through to the other root (fingerprint will reject a divergent file).
    } finally {
      closeSync(fd)
    }
  }
  return null
}
